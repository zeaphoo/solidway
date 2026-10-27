//! Responses request -> Anthropic request。

use std::collections::HashMap;

use serde_json::{Map, Value, json};

mod content;

use crate::common::model::is_reasoning_model;
use crate::common::schema::normalize_object_schema;
use crate::registry::Plan;
use crate::report::{Error, Report, Side};
use crate::wire::anthropic::{
    AnthropicContentBlock, AnthropicMessage, AnthropicMessageContent, AnthropicOutputConfig,
    AnthropicRequest, AnthropicSystem, AnthropicThinking, AnthropicTool,
    AnthropicToolResultContent,
};
use crate::wire::openai_responses::{
    ResponsesContent, ResponsesInput, ResponsesInputItem, ResponsesRequest, ResponsesToolOutput,
};
use content::{content_to_blocks, decode_thinking, empty_block};

const DEFAULT_MAX_TOKENS: u32 = 8192;

pub(super) fn convert(
    req: ResponsesRequest,
    plan: &Plan,
) -> Result<(AnthropicRequest, Report), Error> {
    let mut report = Report::default();
    let mut messages: Vec<AnthropicMessage> = Vec::new();
    let mut developer_system = Vec::new();

    match req.input.as_ref() {
        None => {}
        Some(ResponsesInput::Text(text)) => messages.push(AnthropicMessage {
            role: "user".to_string(),
            content: AnthropicMessageContent::Text(text.clone()),
            extra: Map::new(),
        }),
        Some(ResponsesInput::Items(items)) => {
            for (index, item) in items.iter().enumerate() {
                if is_developer_message(item) {
                    if let Some(text) = responses_message_text(item.content.as_ref())
                        && !text.is_empty()
                    {
                        developer_system.push(text);
                    }
                    continue;
                }
                convert_item(item, index, &mut messages, &mut report, plan)?;
            }
        }
    }

    if messages.is_empty() {
        report.degraded(
            Side::Source,
            "/input",
            "no convertible messages; emitting an empty Anthropic message list",
        );
    }
    let messages = normalize_tool_pairing(merge_assistant_messages(messages));

    let mut tools = Vec::new();
    for (index, tool) in req.tools.iter().enumerate() {
        match tool.type_.as_str() {
            "web_search" | "google_search" | "web_search_20250305" => {
                tools.push(AnthropicTool {
                    type_: Some("web_search_20250305".to_string()),
                    name: "web_search".to_string(),
                    description: None,
                    input_schema: None,
                    cache_control: None,
                    strict: None,
                    extra: Map::new(),
                });
            }
            "function" | "custom" => {
                let Some(name) = tool.name.as_ref() else {
                    continue;
                };
                tools.push(AnthropicTool {
                    type_: None,
                    name: name.clone(),
                    description: tool.description.clone(),
                    input_schema: Some(normalize_object_schema(tool.parameters.as_ref())),
                    cache_control: None,
                    strict: None,
                    extra: Map::new(),
                });
            }
            other => {
                let Some(name) = tool.name.as_ref() else {
                    report.degraded(
                        Side::Source,
                        format!("/tools/{index}/type"),
                        format!("Responses tool {other} has no Anthropic equivalent; dropped"),
                    );
                    continue;
                };
                tools.push(AnthropicTool {
                    type_: Some(other.to_string()),
                    name: name.clone(),
                    description: tool.description.clone(),
                    input_schema: Some(normalize_object_schema(tool.parameters.as_ref())),
                    cache_control: None,
                    strict: None,
                    extra: Map::new(),
                });
            }
        }
    }
    let declared_tools: Vec<String> = tools.iter().map(|tool| tool.name.clone()).collect();
    let tool_choice =
        convert_tool_choice(req.tool_choice.as_ref(), &declared_tools, &mut report, plan)?;

    let max_tokens = req.max_output_tokens.unwrap_or_else(|| {
        report.warn(
            crate::report::WarningCode::Synthesized,
            Side::Target,
            "/max_tokens",
            "Anthropic requires max_tokens; using 8192",
        );
        DEFAULT_MAX_TOKENS
    });

    let thinking = req
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.effort.as_deref())
        .and_then(|effort| {
            let budget = match effort {
                "low" => 1024,
                "medium" => 2048,
                "high" | "xhigh" => 4096,
                _ => 2048,
            };
            if max_tokens <= budget {
                report.degraded(
                    Side::Target,
                    "/thinking",
                    "reasoning effort is dropped because max_tokens is not larger than the thinking budget",
                );
                None
            } else {
                report.degraded(
                    Side::Target,
                    "/thinking",
                    "Responses reasoning effort is mapped to an Anthropic thinking budget",
                );
                Some(AnthropicThinking {
                    type_: "enabled".to_string(),
                    budget_tokens: Some(budget),
                    extra: Map::new(),
                })
            }
        });

    if let Some(text) = req.text.as_ref()
        && text.verbosity.is_some()
    {
        report.degraded(
            Side::Source,
            "/text/verbosity",
            "Anthropic has no verbosity field",
        );
    }
    if let Some(reasoning) = req.reasoning.as_ref() {
        if reasoning.summary.is_some() {
            report.degraded(
                Side::Source,
                "/reasoning/summary",
                "Anthropic thinking has no summary field",
            );
        }
        if reasoning.max_tokens.is_some() {
            report.degraded(
                Side::Source,
                "/reasoning/max_tokens",
                "Anthropic thinking budget is derived from effort, not max_tokens",
            );
        }
    }
    for key in req.extra.keys() {
        report.warn(
            crate::report::WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Responses request field is not mapped to Anthropic",
        );
    }

    let output_config = req.text.as_ref().and_then(|text| {
        text.format.as_ref().map(|format| AnthropicOutputConfig {
            effort: None,
            format: Some(match format.get("schema") {
                Some(schema) => json!({"type": "json_schema", "schema": schema}),
                None => format.clone(),
            }),
            extra: Map::new(),
        })
    });

    for (pointer, present) in [
        ("/store", req.store.is_some()),
        ("/include", !req.include.is_empty()),
        ("/previous_response_id", req.previous_response_id.is_some()),
        ("/conversation", req.conversation.is_some()),
        ("/prompt_cache_key", req.prompt_cache_key.is_some()),
        ("/max_tool_calls", req.max_tool_calls.is_some()),
        ("/safety_identifier", req.safety_identifier.is_some()),
        ("/truncation", req.truncation.is_some()),
        ("/background", req.background.is_some()),
        ("/prompt", req.prompt.is_some()),
        ("/user", req.user.is_some()),
    ] {
        if present {
            report.unsupported(
                plan.policy,
                Side::Source,
                pointer,
                "Responses-only field has no Anthropic equivalent",
            )?;
        }
    }
    if req.parallel_tool_calls.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/parallel_tool_calls",
            "Anthropic has no parallel_tool_calls",
        )?;
    }

    let mut system_text = req.instructions.clone().unwrap_or_default();
    for text in developer_system {
        if !system_text.is_empty() {
            system_text.push('\n');
        }
        system_text.push_str(&text);
    }
    let system = (!system_text.is_empty()).then_some(AnthropicSystem::Text(system_text));

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

    let out = AnthropicRequest {
        model: req.model,
        max_tokens: Some(max_tokens),
        system,
        messages,
        tools,
        stream: req.stream,
        temperature,
        top_p: req.top_p,
        top_k: None,
        stop_sequences: Vec::new(),
        thinking,
        tool_choice,
        metadata: req.metadata,
        output_config,
        service_tier: req.service_tier,
        container: None,
        context_management: None,
        mcp_servers: None,
        extra: Map::new(),
    };

    Ok((out, report))
}

fn merge_assistant_messages(messages: Vec<AnthropicMessage>) -> Vec<AnthropicMessage> {
    let mut out: Vec<AnthropicMessage> = Vec::new();
    for message in messages {
        if message.role == "assistant"
            && let Some(last) = out.last_mut()
            && last.role == "assistant"
        {
            let existing = std::mem::replace(
                &mut last.content,
                AnthropicMessageContent::Text(String::new()),
            );
            last.content = merge_message_content(existing, message.content);
            last.extra.extend(message.extra);
            continue;
        }
        out.push(message);
    }
    out
}

fn merge_message_content(
    existing: AnthropicMessageContent,
    incoming: AnthropicMessageContent,
) -> AnthropicMessageContent {
    match (existing, incoming) {
        (AnthropicMessageContent::Text(a), AnthropicMessageContent::Text(b)) => {
            AnthropicMessageContent::Text(format!("{a}{b}"))
        }
        (a, b) => {
            let mut blocks = content_to_blocks_for_merge(a);
            blocks.extend(content_to_blocks_for_merge(b));
            AnthropicMessageContent::Blocks(blocks)
        }
    }
}

fn content_to_blocks_for_merge(content: AnthropicMessageContent) -> Vec<AnthropicContentBlock> {
    match content {
        AnthropicMessageContent::Text(text) => vec![AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(text),
            ..empty_block()
        }],
        AnthropicMessageContent::Blocks(blocks) => blocks,
    }
}

fn normalize_tool_pairing(messages: Vec<AnthropicMessage>) -> Vec<AnthropicMessage> {
    let mut results: HashMap<String, AnthropicContentBlock> = HashMap::new();
    for message in &messages {
        if message.role != "user" {
            continue;
        }
        for block in message_blocks(&message.content) {
            if block.type_ == "tool_result"
                && let Some(id) = block.tool_use_id.clone()
            {
                results.insert(id, block);
            }
        }
    }

    let mut out = Vec::new();
    for message in messages {
        let blocks = message_blocks(&message.content);
        match message.role.as_str() {
            "assistant" => {
                let mut tool_uses = Vec::new();
                let mut others = Vec::new();
                for block in blocks {
                    if block.type_ == "tool_use" {
                        tool_uses.push(block);
                    } else {
                        others.push(block);
                    }
                }
                if tool_uses.is_empty() {
                    out.push(message);
                    continue;
                }
                let kept: Vec<AnthropicContentBlock> = tool_uses
                    .into_iter()
                    .filter(|block| block.id.as_ref().is_some_and(|id| results.contains_key(id)))
                    .collect();
                if kept.is_empty() {
                    if !others.is_empty() {
                        out.push(anthropic_message("assistant", others));
                    }
                    continue;
                }
                let mut assistant_blocks = others;
                assistant_blocks.extend(kept.iter().cloned());
                out.push(anthropic_message("assistant", assistant_blocks));
                let result_blocks = kept
                    .iter()
                    .filter_map(|block| block.id.as_ref().and_then(|id| results.get(id).cloned()))
                    .collect();
                out.push(anthropic_message("user", result_blocks));
            }
            "user" => {
                let non_result: Vec<AnthropicContentBlock> = blocks
                    .into_iter()
                    .filter(|block| block.type_ != "tool_result")
                    .collect();
                if !non_result.is_empty() {
                    out.push(anthropic_message("user", non_result));
                }
            }
            _ => out.push(message),
        }
    }
    out
}

fn message_blocks(content: &AnthropicMessageContent) -> Vec<AnthropicContentBlock> {
    match content {
        AnthropicMessageContent::Text(text) => vec![AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(text.clone()),
            ..empty_block()
        }],
        AnthropicMessageContent::Blocks(blocks) => blocks.clone(),
    }
}

fn anthropic_message(role: &str, blocks: Vec<AnthropicContentBlock>) -> AnthropicMessage {
    AnthropicMessage {
        role: role.to_string(),
        content: AnthropicMessageContent::Blocks(blocks),
        extra: Map::new(),
    }
}

fn is_developer_message(item: &ResponsesInputItem) -> bool {
    item.type_.as_deref().unwrap_or("message") == "message"
        && item.role.as_deref() == Some("developer")
}

fn responses_message_text(content: Option<&ResponsesContent>) -> Option<String> {
    match content? {
        ResponsesContent::Text(text) => Some(text.clone()),
        ResponsesContent::Parts(parts) => {
            let text = parts
                .iter()
                .filter(|part| matches!(part.type_.as_str(), "input_text" | "output_text" | "text"))
                .filter_map(|part| part.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
    }
}

fn convert_item(
    item: &ResponsesInputItem,
    index: usize,
    messages: &mut Vec<AnthropicMessage>,
    report: &mut Report,
    plan: &Plan,
) -> Result<(), Error> {
    if item.status.is_some() {
        report.degraded(
            Side::Source,
            format!("/input/{index}/status"),
            "Anthropic input messages have no status field",
        );
    }
    if item.id.is_some() {
        report.degraded(
            Side::Source,
            format!("/input/{index}/id"),
            "Anthropic input messages have no Responses item id",
        );
    }
    for key in item.extra.keys() {
        report.warn(
            crate::report::WarningCode::UnknownField,
            Side::Source,
            format!("/input/{index}/{key}"),
            "unknown Responses input field is not mapped to Anthropic",
        );
    }
    match item.type_.as_deref().unwrap_or("message") {
        "message" => {
            let role = match item.role.as_deref().unwrap_or("user") {
                "agent" => {
                    report.degraded(
                        Side::Source,
                        format!("/input/{index}/role"),
                        "agent message is emitted as a user message",
                    );
                    "user"
                }
                other => other,
            };
            let blocks = content_to_blocks(
                item.content.as_ref(),
                role == "assistant",
                report,
                plan,
                &format!("/input/{index}/content"),
            )?;
            if blocks.is_empty() {
                report.degraded(
                    Side::Source,
                    format!("/input/{index}/content"),
                    "message with no convertible content is dropped",
                );
                return Ok(());
            }
            messages.push(AnthropicMessage {
                role: role.to_string(),
                content: AnthropicMessageContent::Blocks(blocks),
                extra: Map::new(),
            });
        }
        "reasoning" => {
            if item.content.is_some() || item.extra.contains_key("content") {
                report.degraded(
                    Side::Source,
                    format!("/input/{index}"),
                    "reasoning item with content is dropped",
                );
                return Ok(());
            }
            let Some(encrypted) = item.encrypted_content.as_deref() else {
                report.degraded(
                    Side::Source,
                    format!("/input/{index}"),
                    "reasoning item without encrypted_content is dropped",
                );
                return Ok(());
            };
            let text = item
                .summary
                .as_ref()
                .map(|summary| {
                    summary
                        .iter()
                        .map(|part| part.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let decoded = decode_thinking(encrypted);
            let thinking = decoded
                .as_ref()
                .and_then(|payload| payload.get("thinking"))
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(ToString::to_string)
                .unwrap_or(text);
            let signature = decoded
                .as_ref()
                .and_then(|payload| payload.get("signature"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let block = AnthropicContentBlock {
                type_: "thinking".to_string(),
                thinking: Some(thinking),
                signature,
                ..empty_block()
            };
            push_assistant_block(messages, block);
        }
        "custom_tool_call" => {
            let input = match item.input.as_ref() {
                Some(Value::String(text)) => json!({"input": text}),
                Some(value) => json!({"input": value}),
                None => json!({"input": ""}),
            };
            push_assistant_block(
                messages,
                AnthropicContentBlock {
                    type_: "tool_use".to_string(),
                    id: item.call_id.clone(),
                    name: item.name.clone(),
                    input: Some(input),
                    ..empty_block()
                },
            );
        }
        "custom_tool_call_output" | "tool_search_output" => {
            let content = match item.output.as_ref() {
                Some(ResponsesToolOutput::Text(text)) => text.clone(),
                Some(ResponsesToolOutput::Parts(parts)) => parts
                    .iter()
                    .filter_map(|part| part.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n"),
                Some(ResponsesToolOutput::Other(value)) => value.to_string(),
                None => item
                    .extra
                    .get("tools")
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            };
            push_user_block(
                messages,
                AnthropicContentBlock {
                    type_: "tool_result".to_string(),
                    tool_use_id: item
                        .call_id
                        .clone()
                        .or_else(|| Some("tool_search".to_string())),
                    content: Some(AnthropicToolResultContent::Text(content)),
                    is_error: Some(false),
                    ..empty_block()
                },
            );
        }
        "web_search_call" | "x_search_call" => {
            report.degraded(
                Side::Source,
                format!("/input/{index}"),
                "server-side search calls are not replayed into Anthropic history",
            );
        }
        "tool_search_call" => {
            let input = match item.arguments.as_ref() {
                Some(Value::String(text)) => {
                    serde_json::from_str(text).unwrap_or_else(|_| json!({"query": text}))
                }
                Some(value) => value.clone(),
                None => json!({}),
            };
            push_assistant_block(
                messages,
                AnthropicContentBlock {
                    type_: "tool_use".to_string(),
                    id: item.call_id.clone(),
                    name: Some("tool_search".to_string()),
                    input: Some(if input.is_object() { input } else { json!({}) }),
                    ..empty_block()
                },
            );
        }
        "function_call" => {
            let input = match item.arguments.as_ref() {
                Some(Value::String(text)) => {
                    serde_json::from_str(text).unwrap_or_else(|_| json!({}))
                }
                Some(value) => value.clone(),
                None => json!({}),
            };
            push_assistant_block(
                messages,
                AnthropicContentBlock {
                    type_: "tool_use".to_string(),
                    id: item.call_id.clone(),
                    name: item.name.clone(),
                    input: Some(if input.is_object() { input } else { json!({}) }),
                    ..empty_block()
                },
            );
        }
        "function_call_output" => {
            let content = match item.output.as_ref() {
                Some(ResponsesToolOutput::Text(text)) => {
                    AnthropicToolResultContent::Text(text.clone())
                }
                Some(ResponsesToolOutput::Parts(parts)) => {
                    let content = ResponsesContent::Parts(parts.clone());
                    AnthropicToolResultContent::Blocks(content_to_blocks(
                        Some(&content),
                        false,
                        report,
                        plan,
                        &format!("/input/{index}/output"),
                    )?)
                }
                Some(ResponsesToolOutput::Other(value)) => {
                    AnthropicToolResultContent::Text(value.to_string())
                }
                None => AnthropicToolResultContent::Text(String::new()),
            };
            push_user_block(
                messages,
                AnthropicContentBlock {
                    type_: "tool_result".to_string(),
                    tool_use_id: item.call_id.clone(),
                    content: Some(content),
                    is_error: Some(false),
                    ..empty_block()
                },
            );
        }
        other => {
            if let Some(text) = responses_message_text(item.content.as_ref()) {
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Text(text),
                    extra: Map::new(),
                });
                report.degraded(
                    Side::Source,
                    format!("/input/{index}/type"),
                    format!("unknown Responses input item {other} is emitted as user text"),
                );
            } else {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("/input/{index}/type"),
                    format!("Responses input item {other} has no Anthropic equivalent"),
                )?;
            }
        }
    }
    Ok(())
}

fn push_assistant_block(messages: &mut Vec<AnthropicMessage>, block: AnthropicContentBlock) {
    if let Some(last) = messages.last_mut()
        && last.role == "assistant"
    {
        match &mut last.content {
            AnthropicMessageContent::Blocks(blocks) => blocks.push(block),
            AnthropicMessageContent::Text(text) => {
                let text = std::mem::take(text);
                last.content = AnthropicMessageContent::Blocks(vec![AnthropicContentBlock {
                    type_: "text".to_string(),
                    text: Some(text),
                    ..empty_block()
                }]);
                if let AnthropicMessageContent::Blocks(blocks) = &mut last.content {
                    blocks.push(block);
                }
            }
        }
        return;
    }
    messages.push(AnthropicMessage {
        role: "assistant".to_string(),
        content: AnthropicMessageContent::Blocks(vec![block]),
        extra: Map::new(),
    });
}

fn push_user_block(messages: &mut Vec<AnthropicMessage>, block: AnthropicContentBlock) {
    if let Some(last) = messages.last_mut()
        && last.role == "user"
    {
        match &mut last.content {
            AnthropicMessageContent::Blocks(blocks) => blocks.push(block),
            AnthropicMessageContent::Text(text) => {
                let text = std::mem::take(text);
                last.content = AnthropicMessageContent::Blocks(vec![AnthropicContentBlock {
                    type_: "text".to_string(),
                    text: Some(text),
                    ..empty_block()
                }]);
                if let AnthropicMessageContent::Blocks(blocks) = &mut last.content {
                    blocks.push(block);
                }
            }
        }
        return;
    }
    messages.push(AnthropicMessage {
        role: "user".to_string(),
        content: AnthropicMessageContent::Blocks(vec![block]),
        extra: Map::new(),
    });
}

fn convert_tool_choice(
    choice: Option<&Value>,
    declared: &[String],
    report: &mut Report,
    plan: &Plan,
) -> Result<Option<Value>, Error> {
    let Some(choice) = choice else {
        return Ok(None);
    };
    let mapped = match choice {
        Value::String(mode) => match mode.as_str() {
            "auto" => Some(json!({"type": "auto"})),
            "none" => Some(json!({"type": "none"})),
            "required" => Some(json!({"type": "any"})),
            _ => None,
        },
        Value::Object(object) => {
            let type_ = object.get("type").and_then(Value::as_str).unwrap_or("");
            let name = match type_ {
                "tool_search" => "tool_search".to_string(),
                "function" | "custom" => object
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        object
                            .get("function")
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("")
                    .to_string(),
                _ => String::new(),
            };
            if name.is_empty() {
                None
            } else if !declared.iter().any(|declared| declared == &name) {
                report.degraded(
                    Side::Source,
                    "/tool_choice",
                    format!("tool_choice references undeclared tool {name}; dropped"),
                );
                None
            } else {
                Some(json!({"type": "tool", "name": name}))
            }
        }
        _ => None,
    };
    if mapped.is_none() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/tool_choice",
            "Responses tool_choice has no Anthropic equivalent",
        )?;
    }
    Ok(mapped)
}
