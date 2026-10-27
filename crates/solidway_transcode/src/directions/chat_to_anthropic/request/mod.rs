//! Chat request -> Anthropic request。

use std::collections::HashSet;

use serde_json::{Map, json};

use crate::common::model::is_reasoning_model;
use crate::common::schema::normalize_object_schema;
use crate::registry::Plan;
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{
    AnthropicContentBlock, AnthropicMessage, AnthropicMessageContent, AnthropicOutputConfig,
    AnthropicRequest, AnthropicSystem, AnthropicThinking, AnthropicTool,
};
use crate::wire::openai_chat::{ChatRequest, ChatStop};

mod content;
mod params;

use content::{append_chat_content, empty_block, parse_tool_arguments, tool_result_block};
use params::{convert_tool_choice, reject_chat_only_fields, warn_unknown};

const DEFAULT_MAX_TOKENS: u32 = 8192;

pub(super) fn convert(req: ChatRequest, plan: &Plan) -> Result<(AnthropicRequest, Report), Error> {
    let mut report = Report::default();

    let mut system_blocks = Vec::new();
    for (index, message) in req.messages.iter().enumerate() {
        if matches!(message.role.as_str(), "system" | "developer") {
            append_chat_content(
                message.content.as_ref(),
                &mut system_blocks,
                &mut report,
                plan,
                &format!("/messages/{index}/content"),
            )?;
        }
    }

    if let Some(instructions) = req.instructions.as_deref().filter(|text| !text.is_empty()) {
        system_blocks.push(AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(instructions.to_string()),
            ..empty_block()
        });
    }

    let mut messages = Vec::new();
    let mut index = 0;
    while index < req.messages.len() {
        let message = &req.messages[index];
        if message.name.is_some() {
            report.degraded(
                Side::Source,
                format!("/messages/{index}/name"),
                "Chat message name has no Anthropic equivalent",
            );
        }
        for key in message.extra.keys() {
            report.warn(
                WarningCode::UnknownField,
                Side::Source,
                format!("/messages/{index}/{key}"),
                "unknown Chat message field is not mapped to Anthropic",
            );
        }
        match message.role.as_str() {
            "system" | "developer" => index += 1,
            "user" => {
                let mut blocks = Vec::new();
                append_chat_content(
                    message.content.as_ref(),
                    &mut blocks,
                    &mut report,
                    plan,
                    &format!("/messages/{index}/content"),
                )?;
                if let Some(refusal) = message.refusal.as_deref().filter(|text| !text.is_empty()) {
                    blocks.push(AnthropicContentBlock {
                        type_: "text".to_string(),
                        text: Some(refusal.to_string()),
                        ..empty_block()
                    });
                    report.degraded(
                        Side::Target,
                        format!("/messages/{index}/refusal"),
                        "Chat refusal is emitted as an Anthropic text block",
                    );
                }
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Blocks(blocks),
                    extra: Map::new(),
                });
                index += 1;
            }
            "assistant" => {
                let mut blocks = Vec::new();
                if let Some(reasoning) = message
                    .reasoning_content
                    .as_deref()
                    .or(message.reasoning.as_deref())
                    .filter(|value| !value.is_empty())
                {
                    blocks.push(AnthropicContentBlock {
                        type_: "text".to_string(),
                        text: Some(format!("<thinking>{reasoning}</thinking>")),
                        ..empty_block()
                    });
                    report.degraded(
                        Side::Source,
                        format!("/messages/{index}/reasoning_content"),
                        "Chat reasoning is replayed inline as <thinking> text",
                    );
                }
                append_chat_content(
                    message.content.as_ref(),
                    &mut blocks,
                    &mut report,
                    plan,
                    &format!("/messages/{index}/content"),
                )?;
                if let Some(refusal) = message.refusal.as_deref().filter(|text| !text.is_empty()) {
                    blocks.push(AnthropicContentBlock {
                        type_: "text".to_string(),
                        text: Some(refusal.to_string()),
                        ..empty_block()
                    });
                    report.degraded(
                        Side::Target,
                        format!("/messages/{index}/refusal"),
                        "Chat refusal is emitted as an Anthropic text block",
                    );
                }
                if let Some(call) = message.function_call.as_ref() {
                    let input = parse_tool_arguments(
                        &call.arguments,
                        &mut report,
                        plan,
                        &format!("/messages/{index}/function_call/arguments"),
                    )?;
                    blocks.push(AnthropicContentBlock {
                        type_: "tool_use".to_string(),
                        id: message
                            .tool_call_id
                            .clone()
                            .or_else(|| Some(format!("toolu_{index}_legacy"))),
                        name: call.name.clone(),
                        input: Some(input),
                        ..empty_block()
                    });
                    report.degraded(
                        Side::Target,
                        format!("/messages/{index}/function_call"),
                        "legacy Chat function_call is emitted as Anthropic tool_use",
                    );
                } else if message.tool_call_id.is_some() {
                    report.degraded(
                        Side::Source,
                        format!("/messages/{index}/tool_call_id"),
                        "assistant tool_call_id has no Anthropic equivalent",
                    );
                }
                for (tool_index, call) in message.tool_calls.iter().enumerate() {
                    if call.type_.as_deref().is_some_and(|kind| kind != "function") {
                        report.degraded(
                            Side::Source,
                            format!("/messages/{index}/tool_calls/{tool_index}/type"),
                            "Chat tool call type other than function has no Anthropic equivalent",
                        );
                    }
                    let input = parse_tool_arguments(
                        &call.function.arguments,
                        &mut report,
                        plan,
                        &format!("/messages/{index}/tool_calls/{tool_index}/function/arguments"),
                    )?;
                    blocks.push(AnthropicContentBlock {
                        type_: "tool_use".to_string(),
                        id: Some(
                            call.id
                                .clone()
                                .unwrap_or_else(|| format!("toolu_{index}_{tool_index}")),
                        ),
                        name: call.function.name.clone(),
                        input: Some(input),
                        ..empty_block()
                    });
                }
                messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: AnthropicMessageContent::Blocks(blocks),
                    extra: Map::new(),
                });
                index += 1;
            }
            "tool" | "function" => {
                let mut blocks = Vec::new();
                while index < req.messages.len()
                    && matches!(req.messages[index].role.as_str(), "tool" | "function")
                {
                    blocks.push(tool_result_block(
                        &req.messages[index],
                        &mut report,
                        plan,
                        index,
                    )?);
                    index += 1;
                }
                messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: AnthropicMessageContent::Blocks(blocks),
                    extra: Map::new(),
                });
            }
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("/messages/{index}/role"),
                    format!("unknown Chat role: {other}"),
                )?;
                index += 1;
            }
        }
    }

    let messages = normalize_tool_pairing(messages);
    if messages.is_empty() {
        return Err(Error::Invalid(
            "Chat request has no convertible messages".to_string(),
        ));
    }

    let mut tools = Vec::new();
    for (tool_index, tool) in req.tools.iter().enumerate() {
        match tool.type_.to_ascii_lowercase().as_str() {
            "web_search" => tools.push(AnthropicTool {
                type_: Some("web_search_20250305".to_string()),
                name: "web_search".to_string(),
                description: None,
                input_schema: None,
                cache_control: None,
                strict: None,
                extra: Map::new(),
            }),
            "x_search" | "code_execution" => report.degraded(
                Side::Source,
                format!("/tools/{tool_index}/type"),
                format!(
                    "Chat tool type {} has no Anthropic equivalent; dropped",
                    tool.type_
                ),
            ),
            "function" | "custom" => {
                let Some(function) = tool.function.as_ref() else {
                    report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!("/tools/{tool_index}/function"),
                        "Chat tool is missing function",
                    )?;
                    continue;
                };
                tools.push(AnthropicTool {
                    type_: None,
                    name: function.name.clone(),
                    description: function.description.clone(),
                    input_schema: Some(normalize_object_schema(function.parameters.as_ref())),
                    cache_control: None,
                    strict: function.strict.or(Some(false)),
                    extra: Map::new(),
                });
            }
            other => report.unsupported(
                plan.policy,
                Side::Source,
                format!("/tools/{tool_index}/type"),
                format!("Chat tool type {other} has no Anthropic equivalent"),
            )?,
        }
    }
    for function in &req.functions {
        tools.push(AnthropicTool {
            type_: None,
            name: function.name.clone(),
            description: function.description.clone(),
            input_schema: Some(normalize_object_schema(function.parameters.as_ref())),
            cache_control: None,
            strict: function.strict.or(Some(false)),
            extra: Map::new(),
        });
        report.degraded(
            Side::Source,
            "/functions",
            "legacy Chat functions are emitted as Anthropic tools",
        );
    }

    let tool_choice = convert_tool_choice(req.tool_choice.as_ref(), &mut report, plan)?;

    let mut max_tokens = req.max_completion_tokens.or(req.max_tokens);
    if max_tokens.is_none() {
        max_tokens = Some(DEFAULT_MAX_TOKENS);
        report.warn(
            WarningCode::Synthesized,
            Side::Target,
            "/max_tokens",
            "Anthropic requires max_tokens; using 8192",
        );
    }
    let max_tokens = max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);

    let stop_sequences = match req.stop.as_ref() {
        Some(ChatStop::One(value)) => vec![value.clone()],
        Some(ChatStop::Many(values)) => values.clone(),
        None => Vec::new(),
    };

    let thinking = req.reasoning_effort.as_deref().and_then(|effort| {
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
                "Chat reasoning_effort is mapped to an Anthropic thinking budget",
            );
            Some(AnthropicThinking {
                type_: "enabled".to_string(),
                budget_tokens: Some(budget),
                extra: Map::new(),
            })
        }
    });

    let output_config = req.response_format.as_ref().map(|format| {
        report.degraded(
            Side::Target,
            "/output_config/format",
            "Chat response_format is mapped to Anthropic output_config.format",
        );
        AnthropicOutputConfig {
            effort: None,
            format: Some(
                format
                    .json_schema
                    .as_ref()
                    .map(|schema| {
                        schema
                            .get("schema")
                            .cloned()
                            .unwrap_or_else(|| schema.clone())
                    })
                    .map(|schema| json!({"type": "json_schema", "schema": schema}))
                    .unwrap_or_else(|| json!({"type": format.type_})),
            ),
            extra: Map::new(),
        }
    });

    reject_chat_only_fields(&req, &mut report, plan)?;
    warn_unknown(&req.extra, &mut report, "/");

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
        system: if system_blocks.is_empty() {
            None
        } else {
            Some(AnthropicSystem::Blocks(system_blocks))
        },
        messages,
        tools,
        stream: req.stream,
        temperature,
        top_p: req.top_p,
        top_k: None,
        stop_sequences,
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

fn normalize_tool_pairing(messages: Vec<AnthropicMessage>) -> Vec<AnthropicMessage> {
    let mut tool_uses = HashSet::new();
    let mut tool_results = HashSet::new();
    for message in &messages {
        if let AnthropicMessageContent::Blocks(blocks) = &message.content {
            for block in blocks {
                match block.type_.as_str() {
                    "tool_use" => {
                        if let Some(id) = block.id.as_deref() {
                            tool_uses.insert(id.to_string());
                        }
                    }
                    "tool_result" => {
                        if let Some(id) = block.tool_use_id.as_deref() {
                            tool_results.insert(id.to_string());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    messages
        .into_iter()
        .filter_map(|mut message| {
            if let AnthropicMessageContent::Blocks(blocks) = &mut message.content {
                blocks.retain(|block| match block.type_.as_str() {
                    "tool_use" => block
                        .id
                        .as_deref()
                        .is_some_and(|id| tool_results.contains(id)),
                    "tool_result" => block
                        .tool_use_id
                        .as_deref()
                        .is_some_and(|id| tool_uses.contains(id)),
                    _ => true,
                });
                if blocks.is_empty() {
                    return None;
                }
            }
            Some(message)
        })
        .collect()
}
