//! Anthropic request -> Chat request。

use serde_json::{Map, Value, json};

mod content;

use crate::common::model::{MIN_OUTPUT_TOKENS, is_reasoning_model};
use crate::common::schema::normalize_object_schema;
use crate::registry::Plan;
use crate::report::{Error, Report, Side};
use crate::wire::anthropic::{
    AnthropicContentBlock, AnthropicMessage, AnthropicMessageContent, AnthropicRequest,
    AnthropicSystem,
};
use crate::wire::openai_chat::{
    ChatContent, ChatContentPart, ChatFunction, ChatFunctionCall, ChatMessage, ChatRequest,
    ChatResponseFormat, ChatStop, ChatTool, ChatToolCall, ChatToolChoice,
};
use content::{
    content_block_to_chat_part, empty_message, empty_part, join_text_blocks, tool_result_messages,
};

pub(super) fn convert(req: AnthropicRequest, plan: &Plan) -> Result<(ChatRequest, Report), Error> {
    let mut report = Report::default();
    let mut messages = Vec::new();

    match req.system.as_ref() {
        None => {}
        Some(AnthropicSystem::Text(text)) if !text.is_empty() && !is_billing_header(text) => {
            messages.push(ChatMessage {
                role: "system".to_string(),
                content: Some(ChatContent::Text(text.clone())),
                ..empty_message()
            })
        }
        Some(AnthropicSystem::Blocks(blocks)) => {
            let text = join_text_blocks(blocks)
                .lines()
                .filter(|line| !is_billing_header(line))
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: Some(ChatContent::Text(text)),
                    ..empty_message()
                });
            }
            if blocks.iter().any(|block| block.cache_control.is_some()) {
                report.degraded(
                    Side::Target,
                    "/system",
                    "Anthropic system cache_control is dropped",
                );
            }
        }
        Some(AnthropicSystem::Text(_)) => {}
    }

    for (index, message) in req.messages.iter().enumerate() {
        for key in message.extra.keys() {
            report.warn(
                crate::report::WarningCode::UnknownField,
                Side::Source,
                format!("/messages/{index}/{key}"),
                "unknown Anthropic message field is not mapped to Chat",
            );
        }
        match message.role.as_str() {
            "user" => convert_user_message(message, &mut messages, &mut report, plan, index)?,
            "assistant" => {
                convert_assistant_message(message, &mut messages, &mut report, plan, index)?
            }
            other => report.unsupported(
                plan.policy,
                Side::Source,
                format!("/messages/{index}/role"),
                format!("unknown Anthropic role: {other}"),
            )?,
        }
    }

    if req.container.is_some() {
        report.degraded(Side::Source, "/container", "Chat has no container field");
    }
    if req.context_management.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/context_management",
            "Chat has no context_management equivalent",
        )?;
    }
    if req.mcp_servers.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/mcp_servers",
            "Chat has no MCP server equivalent",
        )?;
    }
    for key in req.extra.keys() {
        report.warn(
            crate::report::WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Anthropic request field is not mapped to Chat",
        );
    }

    let mut tools = Vec::new();
    for (index, tool) in req.tools.iter().enumerate() {
        if tool.cache_control.is_some() {
            report.degraded(
                Side::Source,
                format!("/tools/{index}/cache_control"),
                "Chat has no cache_control field",
            );
        }
        if let Some(type_) = tool.type_.as_deref()
            && type_.starts_with("web_search")
        {
            report.degraded(
                Side::Source,
                format!("/tools/{index}/type"),
                format!("Anthropic server tool {type_} has no Chat equivalent; dropped"),
            );
            continue;
        }
        tools.push(ChatTool {
            type_: "function".to_string(),
            function: Some(ChatFunction {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: Some(normalize_object_schema(tool.input_schema.as_ref())),
                strict: tool.strict.or(Some(false)),
                extra: Map::new(),
            }),
            extra: Map::new(),
        });
    }

    let declared_tools: Vec<String> = tools
        .iter()
        .filter_map(|tool| tool.function.as_ref().map(|function| function.name.clone()))
        .collect();
    let tool_choice =
        convert_tool_choice(req.tool_choice.as_ref(), &declared_tools, &mut report, plan)?;

    if req.top_k.is_some() {
        report.unsupported(plan.policy, Side::Source, "/top_k", "Chat has no top_k")?;
    }

    let thinking_disabled = req
        .thinking
        .as_ref()
        .is_some_and(|thinking| thinking.type_ == "disabled");
    let mut reasoning_effort = None;
    if thinking_disabled {
        reasoning_effort = Some("none".to_string());
    }
    if let Some(thinking) = req.thinking.as_ref()
        && thinking.type_ != "disabled"
    {
        let effort = thinking
            .budget_tokens
            .map(|budget| match budget {
                0..=1024 => "low",
                1025..=2048 => "medium",
                _ => "high",
            })
            .unwrap_or("medium");
        reasoning_effort = Some(effort.to_string());
        report.degraded(
            Side::Target,
            "/reasoning_effort",
            "Anthropic thinking budget is mapped to Chat reasoning_effort",
        );
    }
    if reasoning_effort.is_none() && !thinking_disabled {
        reasoning_effort = req
            .output_config
            .as_ref()
            .and_then(|config| config.effort.clone())
            .map(|effort| match effort.as_str() {
                "max" => "high".to_string(),
                other => other.to_string(),
            });
    }
    if reasoning_effort.is_none() && !thinking_disabled && is_reasoning_model(&req.model) {
        reasoning_effort = Some("medium".to_string());
    }

    let stop = if req.stop_sequences.is_empty() {
        None
    } else {
        Some(ChatStop::Many(req.stop_sequences.clone()))
    };

    let response_format = req.output_config.as_ref().and_then(|config| {
        config.format.as_ref().map(|format| ChatResponseFormat {
            type_: format
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("json_schema")
                .to_string(),
            json_schema: format.get("schema").map(|schema| {
                json!({
                    "name": format.get("name").cloned().unwrap_or_else(|| json!("response")),
                    "schema": schema
                })
            }),
            extra: Map::new(),
        })
    });

    let temperature = if req.temperature.is_some() && is_reasoning_model(&req.model) {
        report.degraded(
            Side::Source,
            "/temperature",
            "reasoning models reject temperature; parameter is dropped",
        );
        None
    } else {
        req.temperature
    };

    let out = ChatRequest {
        model: req.model,
        messages,
        instructions: None,
        max_tokens: None,
        max_completion_tokens: req.max_tokens.map(|value| {
            if value < MIN_OUTPUT_TOKENS {
                report.degraded(
                    Side::Source,
                    "/max_completion_tokens",
                    format!("bumped to the minimum of {MIN_OUTPUT_TOKENS}"),
                );
                MIN_OUTPUT_TOKENS
            } else {
                value
            }
        }),
        temperature,
        top_p: req.top_p,
        stream: req.stream,
        stream_options: None,
        tools,
        parallel_tool_calls: None,
        tool_choice,
        reasoning_effort,
        service_tier: req.service_tier,
        stop,
        response_format,
        functions: Vec::new(),
        function_call: None,
        n: None,
        seed: None,
        presence_penalty: None,
        frequency_penalty: None,
        logprobs: None,
        top_logprobs: None,
        user: None,
        metadata: req.metadata,
        extra: Map::new(),
    };

    Ok((out, report))
}

fn convert_user_message(
    message: &AnthropicMessage,
    out: &mut Vec<ChatMessage>,
    report: &mut Report,
    plan: &Plan,
    index: usize,
) -> Result<(), Error> {
    match &message.content {
        AnthropicMessageContent::Text(text) => out.push(ChatMessage {
            role: "user".to_string(),
            content: Some(ChatContent::Text(text.clone())),
            ..empty_message()
        }),
        AnthropicMessageContent::Blocks(blocks) => {
            let mut parts = Vec::new();
            let mut tool_results = Vec::new();
            for (block_index, block) in blocks.iter().enumerate() {
                warn_block_fields(block, index, block_index, report);
                match block.type_.as_str() {
                    "tool_result" => tool_results.extend(tool_result_messages(
                        block,
                        report,
                        plan,
                        index,
                        block_index,
                    )?),
                    "thinking" | "redacted_thinking" => {
                        report.degraded(
                            Side::Target,
                            format!("/messages/{index}/content/{block_index}"),
                            "thinking blocks in user messages are dropped",
                        );
                    }
                    _ => {
                        if let Some(part) = content_block_to_chat_part(
                            block,
                            report,
                            plan,
                            &format!("/messages/{index}/content/{block_index}"),
                        )? {
                            parts.push(part);
                        }
                    }
                }
            }
            if !parts.is_empty() {
                let content = if parts.iter().all(|part| part.type_ == "text") {
                    ChatContent::Text(
                        parts
                            .iter()
                            .filter_map(|part| part.text.as_deref())
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                    )
                } else {
                    ChatContent::Parts(parts)
                };
                out.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(content),
                    ..empty_message()
                });
            }
            out.extend(tool_results);
        }
    }
    Ok(())
}

fn convert_assistant_message(
    message: &AnthropicMessage,
    out: &mut Vec<ChatMessage>,
    report: &mut Report,
    plan: &Plan,
    index: usize,
) -> Result<(), Error> {
    let mut parts = Vec::new();
    let mut reasoning = Vec::new();
    let mut tool_calls = Vec::new();

    match &message.content {
        AnthropicMessageContent::Text(text) => {
            if !text.is_empty() {
                parts.push(ChatContentPart {
                    type_: "text".to_string(),
                    text: Some(text.clone()),
                    ..empty_part()
                });
            }
        }
        AnthropicMessageContent::Blocks(blocks) => {
            for (block_index, block) in blocks.iter().enumerate() {
                warn_block_fields(block, index, block_index, report);
                match block.type_.as_str() {
                    "thinking" => {
                        if let Some(thinking) = block.thinking.as_deref() {
                            reasoning.push(thinking.to_string());
                        }
                        report.degraded(
                            Side::Target,
                            format!("/messages/{index}/content/{block_index}/signature"),
                            "Anthropic thinking signature has no Chat field",
                        );
                    }
                    "redacted_thinking" => {
                        report.degraded(
                            Side::Target,
                            format!("/messages/{index}/content/{block_index}"),
                            "Anthropic redacted_thinking has no Chat equivalent",
                        );
                    }
                    "tool_use" => {
                        let input = block.input.clone().unwrap_or_else(|| json!({}));
                        tool_calls.push(ChatToolCall {
                            index: None,
                            id: block.id.clone(),
                            type_: Some("function".to_string()),
                            function: ChatFunctionCall {
                                name: block.name.clone(),
                                arguments: serde_json::to_string(&input)?,
                                extra: Map::new(),
                            },
                            extra: Map::new(),
                        });
                    }
                    _ => {
                        if let Some(part) = content_block_to_chat_part(
                            block,
                            report,
                            plan,
                            &format!("/messages/{index}/content/{block_index}"),
                        )? {
                            parts.push(part);
                        }
                    }
                }
            }
        }
    }

    let content = if parts.is_empty() {
        if tool_calls.is_empty() {
            Some(ChatContent::Text(String::new()))
        } else {
            None
        }
    } else if parts.len() == 1 && parts[0].type_ == "text" {
        parts[0]
            .text
            .clone()
            .map(ChatContent::Text)
            .or(Some(ChatContent::Parts(parts)))
    } else {
        Some(ChatContent::Parts(parts))
    };

    out.push(ChatMessage {
        role: "assistant".to_string(),
        content,
        reasoning_content: if reasoning.is_empty() || tool_calls.is_empty() {
            if !reasoning.is_empty() {
                report.degraded(
                    Side::Source,
                    "/messages/reasoning",
                    "thinking is only replayed on tool-call turns; dropped",
                );
            }
            None
        } else {
            Some(reasoning.join("\n"))
        },
        tool_calls,
        ..empty_message()
    });
    Ok(())
}

fn warn_block_fields(
    block: &AnthropicContentBlock,
    message_index: usize,
    block_index: usize,
    report: &mut Report,
) {
    if block.cache_control.is_some() {
        report.degraded(
            Side::Source,
            format!("/messages/{message_index}/content/{block_index}/cache_control"),
            "Chat has no cache_control field",
        );
    }
    if !block.citations.is_empty() {
        report.degraded(
            Side::Source,
            format!("/messages/{message_index}/content/{block_index}/citations"),
            "Chat has no Anthropic citations field",
        );
    }
    for key in block.extra.keys() {
        report.warn(
            crate::report::WarningCode::UnknownField,
            Side::Source,
            format!("/messages/{message_index}/content/{block_index}/{key}"),
            "unknown Anthropic content field is not mapped to Chat",
        );
    }
}

fn is_billing_header(text: &str) -> bool {
    text.starts_with("x-anthropic-billing-header: ")
}

fn convert_tool_choice(
    choice: Option<&Value>,
    declared_tools: &[String],
    report: &mut Report,
    plan: &Plan,
) -> Result<Option<ChatToolChoice>, Error> {
    let Some(choice) = choice else {
        return Ok(None);
    };
    let type_ = choice.get("type").and_then(Value::as_str).unwrap_or("");
    match type_ {
        "auto" => Ok(Some(ChatToolChoice::Mode("auto".to_string()))),
        "none" => Ok(Some(ChatToolChoice::Mode("none".to_string()))),
        "any" => Ok(Some(ChatToolChoice::Mode("required".to_string()))),
        "tool" => {
            if let Some(name) = choice.get("name").and_then(Value::as_str) {
                if !declared_tools.iter().any(|declared| declared == name) {
                    report.degraded(
                        Side::Source,
                        "/tool_choice",
                        format!("tool_choice references undeclared tool {name}; dropped"),
                    );
                    return Ok(None);
                }
                let mut object = Map::new();
                object.insert("type".to_string(), json!("function"));
                object.insert("function".to_string(), json!({"name": name}));
                Ok(Some(ChatToolChoice::Object(object)))
            } else {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    "/tool_choice/name",
                    "Anthropic tool_choice tool is missing name",
                )?;
                Ok(None)
            }
        }
        other => {
            report.unsupported(
                plan.policy,
                Side::Source,
                "/tool_choice/type",
                format!("unknown Anthropic tool_choice type: {other}"),
            )?;
            Ok(None)
        }
    }
}
