//! Anthropic request -> Responses request。

use serde_json::{Map, Value, json};

use crate::common::model::{MIN_OUTPUT_TOKENS, is_reasoning_model};
use crate::common::schema::normalize_object_schema;
use crate::registry::Plan;
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{
    AnthropicContentBlock, AnthropicMessage, AnthropicMessageContent, AnthropicRequest,
    AnthropicSystem, AnthropicToolResultContent,
};
use crate::wire::openai_responses::{
    ResponsesContent, ResponsesContentPart, ResponsesImageUrl, ResponsesInput, ResponsesInputItem,
    ResponsesReasoning, ResponsesRequest, ResponsesSummary, ResponsesText, ResponsesTool,
    ResponsesToolOutput,
};

pub(super) fn convert(
    req: AnthropicRequest,
    plan: &Plan,
) -> Result<(ResponsesRequest, Report), Error> {
    let mut report = Report::default();
    let system_parts: Vec<String> = match req.system.as_ref() {
        None => Vec::new(),
        Some(AnthropicSystem::Text(text)) => {
            if text.is_empty() || is_billing_header(text) {
                Vec::new()
            } else {
                vec![text.clone()]
            }
        }
        Some(AnthropicSystem::Blocks(blocks)) => {
            if blocks.iter().any(|block| block.cache_control.is_some()) {
                report.degraded(
                    Side::Source,
                    "/system",
                    "Anthropic system cache_control has no Responses equivalent",
                );
            }
            blocks
                .iter()
                .filter(|block| block.type_ == "text")
                .filter_map(|block| block.text.as_deref())
                .filter(|text| !is_billing_header(text))
                .map(ToString::to_string)
                .collect()
        }
    };

    let mut input = Vec::new();
    if !system_parts.is_empty() {
        input.push(message_item(
            "developer",
            ResponsesContent::Parts(
                system_parts
                    .into_iter()
                    .map(|text| ResponsesContentPart {
                        type_: "input_text".to_string(),
                        text: Some(text),
                        ..empty_part()
                    })
                    .collect(),
            ),
        ));
    }
    for (index, message) in req.messages.iter().enumerate() {
        convert_message(message, index, &mut input, &mut report, plan)?;
    }
    if input.is_empty() {
        return Err(Error::Invalid(
            "Anthropic request has no convertible input".to_string(),
        ));
    }

    if req.container.is_some() {
        report.degraded(
            Side::Source,
            "/container",
            "Responses has no container field",
        );
    }
    if req.context_management.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/context_management",
            "Responses has no context_management equivalent",
        )?;
    }
    if req.mcp_servers.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/mcp_servers",
            "Responses has no MCP server equivalent",
        )?;
    }
    for key in req.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Anthropic request field is not mapped to Responses",
        );
    }

    let mut tools = Vec::new();
    for (index, tool) in req.tools.iter().enumerate() {
        if tool.cache_control.is_some() {
            report.degraded(
                Side::Source,
                format!("/tools/{index}/cache_control"),
                "Responses has no cache_control field",
            );
        }
        if let Some(type_) = tool.type_.as_deref()
            && type_.starts_with("web_search")
        {
            tools.push(ResponsesTool {
                type_: "web_search".to_string(),
                name: Some("web_search".to_string()),
                description: None,
                parameters: None,
                strict: None,
                format: None,
                tools: Vec::new(),
                children: Vec::new(),
                extra: Map::new(),
            });
            continue;
        }
        tools.push(ResponsesTool {
            type_: "function".to_string(),
            name: Some(tool.name.clone()),
            description: tool.description.clone(),
            parameters: Some(normalize_object_schema(tool.input_schema.as_ref())),
            strict: tool.strict.or(Some(false)),
            format: None,
            tools: Vec::new(),
            children: Vec::new(),
            extra: Map::new(),
        });
    }

    let declared_tools: Vec<String> = tools.iter().filter_map(|tool| tool.name.clone()).collect();
    let tool_choice =
        convert_tool_choice(req.tool_choice.as_ref(), &declared_tools, &mut report, plan)?;

    if req.top_k.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/top_k",
            "Responses has no top_k",
        )?;
    }
    if !req.stop_sequences.is_empty() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/stop_sequences",
            "Responses has no stop parameter",
        )?;
    }

    let thinking_disabled = req
        .thinking
        .as_ref()
        .is_some_and(|thinking| thinking.type_ == "disabled");
    let reasoning = if thinking_disabled {
        None
    } else {
        req.thinking
            .as_ref()
            .filter(|thinking| thinking.type_ != "disabled")
            .map(|thinking| ResponsesReasoning {
                effort: Some(
                    match thinking.budget_tokens.unwrap_or(2048) {
                        0..=1024 => "low",
                        1025..=2048 => "medium",
                        _ => "high",
                    }
                    .to_string(),
                ),
                summary: Some("auto".to_string()),
                max_tokens: thinking.budget_tokens,
                extra: Map::new(),
            })
            .or_else(|| {
                req.output_config
                    .as_ref()
                    .and_then(|config| config.effort.clone())
                    .map(|effort| ResponsesReasoning {
                        effort: Some(match effort.as_str() {
                            "max" => "high".to_string(),
                            other => other.to_string(),
                        }),
                        summary: Some("auto".to_string()),
                        max_tokens: None,
                        extra: Map::new(),
                    })
            })
    };

    let text = req.output_config.as_ref().and_then(|config| {
        config.format.as_ref().map(|format| ResponsesText {
            format: Some(format.clone()),
            verbosity: None,
            extra: Map::new(),
        })
    });

    let include = if req.messages.iter().any(has_thinking) {
        vec!["reasoning.encrypted_content".to_string()]
    } else {
        Vec::new()
    };

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

    let max_output_tokens = req.max_tokens.map(|value| {
        if value < MIN_OUTPUT_TOKENS {
            report.degraded(
                Side::Source,
                "/max_output_tokens",
                format!("bumped to the minimum of {MIN_OUTPUT_TOKENS}"),
            );
            MIN_OUTPUT_TOKENS
        } else {
            value
        }
    });

    let out = ResponsesRequest {
        model: req.model,
        instructions: None,
        input: Some(ResponsesInput::Items(input)),
        max_output_tokens,
        temperature,
        top_p: req.top_p,
        stream: req.stream,
        tools,
        include,
        store: Some(false),
        parallel_tool_calls: None,
        reasoning,
        text,
        tool_choice,
        service_tier: req.service_tier,
        prompt_cache_key: None,
        previous_response_id: None,
        metadata: req.metadata,
        conversation: None,
        max_tool_calls: None,
        safety_identifier: None,
        truncation: None,
        background: None,
        prompt: None,
        user: None,
        extra: Map::new(),
    };

    Ok((out, report))
}

fn is_billing_header(text: &str) -> bool {
    text.starts_with("x-anthropic-billing-header: ")
}

fn has_thinking(message: &AnthropicMessage) -> bool {
    match &message.content {
        AnthropicMessageContent::Text(_) => false,
        AnthropicMessageContent::Blocks(blocks) => blocks
            .iter()
            .any(|block| matches!(block.type_.as_str(), "thinking" | "redacted_thinking")),
    }
}

fn convert_message(
    message: &AnthropicMessage,
    index: usize,
    input: &mut Vec<ResponsesInputItem>,
    report: &mut Report,
    plan: &Plan,
) -> Result<(), Error> {
    for key in message.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/messages/{index}/{key}"),
            "unknown Anthropic message field is not mapped to Responses",
        );
    }
    match &message.content {
        AnthropicMessageContent::Text(text) => {
            let type_ = if message.role == "assistant" {
                "output_text"
            } else {
                "input_text"
            };
            input.push(message_item(
                &message.role,
                ResponsesContent::Parts(vec![ResponsesContentPart {
                    type_: type_.to_string(),
                    text: Some(text.clone()),
                    ..empty_part()
                }]),
            ));
        }
        AnthropicMessageContent::Blocks(blocks) => {
            if message.role == "assistant" {
                for (block_index, block) in blocks.iter().enumerate() {
                    warn_block_fields(block, index, block_index, report);
                    match block.type_.as_str() {
                        "thinking" | "redacted_thinking" => {
                            if let Some(item) = reasoning_item(block, report, index, block_index) {
                                input.push(item);
                            }
                        }
                        "tool_use" | "server_tool_use" => input.push(function_call_item(block)?),
                        "web_search_tool_result" => {
                            input.extend(tool_result_items(
                                block,
                                report,
                                plan,
                                index,
                                block_index,
                            )?);
                        }
                        _ => {
                            if let Some(mut content) =
                                content_block(block, report, plan, index, block_index)?
                            {
                                for part in &mut content {
                                    if part.type_ == "input_text" {
                                        part.type_ = "output_text".to_string();
                                    }
                                }
                                input.push(message_item(
                                    "assistant",
                                    ResponsesContent::Parts(content),
                                ));
                            }
                        }
                    }
                }
            } else {
                let mut parts = Vec::new();
                for (block_index, block) in blocks.iter().enumerate() {
                    warn_block_fields(block, index, block_index, report);
                    match block.type_.as_str() {
                        "tool_result" => input.extend(tool_result_items(
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
                            if let Some(mut content) =
                                content_block(block, report, plan, index, block_index)?
                            {
                                parts.append(&mut content);
                            }
                        }
                    }
                }
                if !parts.is_empty() {
                    input.push(message_item("user", ResponsesContent::Parts(parts)));
                }
            }
        }
    }
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
            "Responses has no cache_control field",
        );
    }
    if !block.citations.is_empty() {
        report.degraded(
            Side::Source,
            format!("/messages/{message_index}/content/{block_index}/citations"),
            "Responses has no Anthropic citations block",
        );
    }
    for key in block.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/messages/{message_index}/content/{block_index}/{key}"),
            "unknown Anthropic content field is not mapped to Responses",
        );
    }
}

fn message_item(role: &str, content: ResponsesContent) -> ResponsesInputItem {
    ResponsesInputItem {
        type_: Some("message".to_string()),
        role: Some(role.to_string()),
        content: Some(content),
        ..empty_item()
    }
}

fn reasoning_item(
    block: &AnthropicContentBlock,
    report: &mut Report,
    message_index: usize,
    block_index: usize,
) -> Option<ResponsesInputItem> {
    if block.signature.is_none() && block.data.is_none() {
        report.degraded(
            Side::Source,
            format!("/messages/{message_index}/content/{block_index}"),
            "Anthropic thinking without signature is dropped",
        );
        return None;
    }
    let text = block
        .thinking
        .clone()
        .or_else(|| block.data.clone())
        .unwrap_or_default();
    if block.signature.is_some() {
        report.warn(
            crate::report::WarningCode::Degraded,
            Side::Target,
            format!("/messages/{message_index}/content/{block_index}/signature"),
            "Anthropic thinking signature is carried as an opaque Responses encrypted_content envelope",
        );
    }
    Some(ResponsesInputItem {
        type_: Some("reasoning".to_string()),
        encrypted_content: Some(encode_thinking(block)),
        summary: Some(vec![ResponsesSummary {
            type_: "summary_text".to_string(),
            text,
            extra: Map::new(),
        }]),
        ..empty_item()
    })
}

fn function_call_item(block: &AnthropicContentBlock) -> Result<ResponsesInputItem, Error> {
    Ok(ResponsesInputItem {
        type_: Some("function_call".to_string()),
        call_id: block.id.clone(),
        name: block.name.clone(),
        arguments: Some(Value::String(serde_json::to_string(
            &block.input.clone().unwrap_or_else(|| json!({})),
        )?)),
        ..empty_item()
    })
}

fn tool_result_items(
    block: &AnthropicContentBlock,
    report: &mut Report,
    plan: &Plan,
    message_index: usize,
    block_index: usize,
) -> Result<Vec<ResponsesInputItem>, Error> {
    let mut text_parts = Vec::new();
    let mut media_parts = Vec::new();
    let output = match block.content.as_ref() {
        Some(AnthropicToolResultContent::Text(text)) => ResponsesToolOutput::Text(text.clone()),
        Some(AnthropicToolResultContent::Blocks(blocks)) => {
            for (part_index, part) in blocks.iter().enumerate() {
                match part.type_.as_str() {
                    "text" => text_parts.push(ResponsesContentPart {
                        type_: "output_text".to_string(),
                        text: part.text.clone(),
                        ..empty_part()
                    }),
                    "image" | "document" => {
                        if let Some(parts) = content_block(
                            part,
                            report,
                            plan,
                            message_index,
                            block_index,
                        )? {
                            media_parts.extend(parts);
                        }
                    }
                    other => report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!(
                            "/messages/{message_index}/content/{block_index}/content/{part_index}/type"
                        ),
                        format!("tool_result block {other} has no Responses equivalent"),
                    )?,
                }
            }
            if text_parts.is_empty() {
                ResponsesToolOutput::Text(String::new())
            } else {
                ResponsesToolOutput::Parts(text_parts)
            }
        }
        Some(AnthropicToolResultContent::Other(value)) => ResponsesToolOutput::Other(value.clone()),
        None => ResponsesToolOutput::Text(String::new()),
    };

    let mut items = vec![ResponsesInputItem {
        type_: Some("function_call_output".to_string()),
        call_id: block.tool_use_id.clone(),
        output: Some(output),
        ..empty_item()
    }];
    if !media_parts.is_empty() {
        items.push(message_item("user", ResponsesContent::Parts(media_parts)));
        report.degraded(
            Side::Target,
            format!("/messages/{message_index}/content/{block_index}/content"),
            "tool_result media is lifted into a separate user message",
        );
    }
    Ok(items)
}

fn content_block(
    block: &AnthropicContentBlock,
    report: &mut Report,
    plan: &Plan,
    message_index: usize,
    block_index: usize,
) -> Result<Option<Vec<ResponsesContentPart>>, Error> {
    match block.type_.as_str() {
        "text" => Ok(Some(vec![ResponsesContentPart {
            type_: "input_text".to_string(),
            text: block.text.clone(),
            ..empty_part()
        }])),
        "image" => {
            let Some(source) = block.source.as_ref() else {
                return Ok(None);
            };
            let url = match source.type_.as_str() {
                "base64" => format!(
                    "data:{};base64,{}",
                    source.media_type.as_deref().unwrap_or("image/png"),
                    source.data.as_deref().unwrap_or_default()
                ),
                "url" => source.url.clone().unwrap_or_default(),
                _ => {
                    report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!("/messages/{message_index}/content/{block_index}/source/type"),
                        "Anthropic image source has no Responses equivalent",
                    )?;
                    return Ok(None);
                }
            };
            Ok(Some(vec![ResponsesContentPart {
                type_: "input_image".to_string(),
                image_url: Some(ResponsesImageUrl::Url(url)),
                ..empty_part()
            }]))
        }
        "document" => {
            let Some(source) = block.source.as_ref() else {
                return Ok(None);
            };
            if source.type_ == "text" {
                report.degraded(
                    Side::Target,
                    "/messages",
                    "Anthropic text document is emitted as Responses input_text",
                );
                return Ok(Some(vec![ResponsesContentPart {
                    type_: "input_text".to_string(),
                    text: source.data.clone(),
                    ..empty_part()
                }]));
            }
            let (file_data, file_id) = match source.type_.as_str() {
                "base64" => (
                    Some(format!(
                        "data:{};base64,{}",
                        source.media_type.as_deref().unwrap_or("application/pdf"),
                        source.data.as_deref().unwrap_or_default()
                    )),
                    None,
                ),
                "file" => (None, source.file_id.clone()),
                _ => (None, None),
            };
            Ok(Some(vec![ResponsesContentPart {
                type_: "input_file".to_string(),
                file_data,
                file_id,
                ..empty_part()
            }]))
        }
        other => {
            report.unsupported(
                plan.policy,
                Side::Source,
                format!("/messages/{message_index}/content/{block_index}/type"),
                format!("Anthropic content block {other} has no Responses equivalent"),
            )?;
            Ok(None)
        }
    }
}

fn empty_item() -> ResponsesInputItem {
    ResponsesInputItem {
        type_: None,
        role: None,
        content: None,
        encrypted_content: None,
        call_id: None,
        name: None,
        arguments: None,
        id: None,
        input: None,
        output: None,
        summary: None,
        status: None,
        extra: Map::new(),
    }
}

fn empty_part() -> ResponsesContentPart {
    ResponsesContentPart {
        type_: String::new(),
        text: None,
        refusal: None,
        image_url: None,
        detail: None,
        filename: None,
        file_data: None,
        file_id: None,
        annotations: Vec::new(),
        logprobs: None,
        prompt_cache_breakpoint: None,
        extra: Map::new(),
    }
}

fn encode_thinking(block: &AnthropicContentBlock) -> String {
    block
        .signature
        .clone()
        .or_else(|| block.data.clone())
        .unwrap_or_default()
}

fn convert_tool_choice(
    choice: Option<&Value>,
    declared_tools: &[String],
    report: &mut Report,
    plan: &Plan,
) -> Result<Option<Value>, Error> {
    let Some(choice) = choice else {
        return Ok(None);
    };
    let type_ = choice.get("type").and_then(Value::as_str).unwrap_or("");
    let mapped = match type_ {
        "auto" => Some(json!("auto")),
        "none" => Some(json!("none")),
        "any" => Some(json!("required")),
        "tool" => choice.get("name").and_then(Value::as_str).and_then(|name| {
            if declared_tools.iter().any(|declared| declared == name) {
                Some(json!({"type": "function", "name": name}))
            } else {
                report.degraded(
                    Side::Source,
                    "/tool_choice",
                    format!("tool_choice references undeclared tool {name}; dropped"),
                );
                None
            }
        }),
        _ => None,
    };
    if mapped.is_none() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/tool_choice",
            "Anthropic tool_choice has no Responses equivalent",
        )?;
    }
    Ok(mapped)
}
