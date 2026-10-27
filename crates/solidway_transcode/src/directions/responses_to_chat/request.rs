//! Responses request -> Chat request。

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value, json};

use crate::common::model::is_reasoning_model;
use crate::common::schema::{custom_tool_input_schema, flatten_namespace_name};
use crate::registry::Plan;
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::openai_chat::{
    ChatContent, ChatContentPart, ChatFile, ChatFunction, ChatFunctionCall, ChatImageUrl,
    ChatMessage, ChatRequest, ChatResponseFormat, ChatTool, ChatToolCall, ChatToolChoice,
};
use crate::wire::openai_responses::{
    ResponsesContent, ResponsesImageUrl, ResponsesInput, ResponsesInputItem, ResponsesRequest,
    ResponsesTool, ResponsesToolOutput,
};

pub(super) fn convert(req: ResponsesRequest, plan: &Plan) -> Result<(ChatRequest, Report), Error> {
    let mut report = Report::default();
    let mut messages = Vec::new();

    if let Some(instructions) = req.instructions.as_ref().filter(|text| !text.is_empty()) {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(ChatContent::Text(instructions.clone())),
            ..empty_message()
        });
    }

    match req.input.as_ref() {
        None => {}
        Some(ResponsesInput::Text(text)) => messages.push(ChatMessage {
            role: "user".to_string(),
            content: Some(ChatContent::Text(text.clone())),
            ..empty_message()
        }),
        Some(ResponsesInput::Items(items)) => {
            for (index, item) in items.iter().enumerate() {
                convert_input_item(item, index, &mut messages, &mut report, plan)?;
            }
        }
    }

    if messages.is_empty() {
        return Err(Error::Invalid(
            "Responses request has no convertible input".to_string(),
        ));
    }
    let messages = merge_assistant_messages(messages);

    let mut tool_inputs = req.tools.clone();
    promote_tool_search_discoveries(&mut tool_inputs, req.input.as_ref());
    promote_additional_tools(&mut tool_inputs, req.input.as_ref());
    flatten_additional_tools(&mut tool_inputs);
    let tools = convert_tools(&tool_inputs, &mut report, plan)?;
    let declared: Vec<String> = tools
        .iter()
        .flat_map(|tool| {
            if tool.type_ == "x_search" {
                vec!["x_search".to_string()]
            } else {
                tool.function
                    .as_ref()
                    .map(|function| function.name.clone())
                    .into_iter()
                    .collect()
            }
        })
        .collect();
    let tool_choice = if tools.is_empty() {
        None
    } else {
        convert_tool_choice(req.tool_choice.as_ref(), &declared, &mut report, plan)?
    };

    if let Some(text) = req.text.as_ref()
        && text.verbosity.is_some()
    {
        report.degraded(
            Side::Source,
            "/text/verbosity",
            "Chat has no verbosity field",
        );
    }
    for key in req.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Responses request field is not mapped to Chat",
        );
    }

    let response_format = req.text.as_ref().and_then(|text| {
        text.format.as_ref().map(|format| ChatResponseFormat {
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

    let reasoning_effort = req
        .reasoning
        .as_ref()
        .and_then(|reasoning| reasoning.effort.clone());

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
                "Responses-only field has no Chat equivalent",
            )?;
        }
    }
    if let Some(reasoning) = req.reasoning.as_ref() {
        if reasoning.summary.is_some() {
            report.degraded(
                Side::Source,
                "/reasoning/summary",
                "Responses reasoning summary is dropped; Chat has no summary field",
            );
        }
        if reasoning.max_tokens.is_some() {
            report.degraded(
                Side::Source,
                "/reasoning/max_tokens",
                "Responses reasoning max_tokens is dropped",
            );
        }
    }

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
        max_completion_tokens: req.max_output_tokens,
        temperature,
        top_p: req.top_p,
        stream: req.stream,
        stream_options: None,
        tools,
        parallel_tool_calls: req.parallel_tool_calls,
        tool_choice,
        reasoning_effort,
        service_tier: req.service_tier,
        stop: None,
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

fn merge_assistant_messages(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let mut out: Vec<ChatMessage> = Vec::new();
    for message in messages {
        if message.role == "assistant"
            && let Some(last) = out.last_mut()
            && last.role == "assistant"
        {
            last.content = merge_content(last.content.take(), message.content);
            if last.reasoning_content.is_none() {
                last.reasoning_content = message.reasoning_content;
            }
            last.tool_calls.extend(message.tool_calls);
            last.extra.extend(message.extra);
            continue;
        }
        out.push(message);
    }
    out
}

fn merge_content(
    existing: Option<ChatContent>,
    incoming: Option<ChatContent>,
) -> Option<ChatContent> {
    match (existing, incoming) {
        (None, incoming) => incoming,
        (existing, None) => existing,
        (Some(ChatContent::Text(a)), Some(ChatContent::Text(b))) => {
            Some(ChatContent::Text(format!("{a}{b}")))
        }
        (Some(a), Some(b)) => {
            let mut parts = Vec::new();
            push_content_parts(&mut parts, a);
            push_content_parts(&mut parts, b);
            Some(ChatContent::Parts(parts))
        }
    }
}

fn push_content_parts(out: &mut Vec<ChatContentPart>, content: ChatContent) {
    match content {
        ChatContent::Text(text) => out.push(ChatContentPart {
            type_: "text".to_string(),
            text: Some(text),
            ..empty_part()
        }),
        ChatContent::Parts(parts) => out.extend(parts),
    }
}

fn empty_message() -> ChatMessage {
    ChatMessage {
        role: String::new(),
        content: None,
        refusal: None,
        reasoning_content: None,
        reasoning: None,
        name: None,
        tool_calls: Vec::new(),
        tool_call_id: None,
        function_call: None,
        extra: Map::new(),
    }
}

fn valid_chat_audio(audio: &crate::wire::openai_chat::ChatInputAudio) -> bool {
    audio.data.as_deref().is_some_and(|data| !data.is_empty())
        && audio
            .format
            .as_deref()
            .is_some_and(|format| matches!(format, "wav" | "mp3"))
}

fn empty_part() -> ChatContentPart {
    ChatContentPart {
        type_: String::new(),
        text: None,
        image_url: None,
        input_audio: None,
        file: None,
        refusal: None,
        prompt_cache_breakpoint: None,
        extra: Map::new(),
    }
}

fn convert_input_item(
    item: &ResponsesInputItem,
    index: usize,
    messages: &mut Vec<ChatMessage>,
    report: &mut Report,
    plan: &Plan,
) -> Result<(), Error> {
    if item.status.is_some() {
        report.degraded(
            Side::Source,
            format!("/input/{index}/status"),
            "Chat input messages have no status field",
        );
    }
    if item.id.is_some() {
        report.degraded(
            Side::Source,
            format!("/input/{index}/id"),
            "Chat input messages have no Responses item id",
        );
    }
    for key in item.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/input/{index}/{key}"),
            "unknown Responses input field is not mapped to Chat",
        );
    }
    match item.type_.as_deref().unwrap_or("message") {
        "agent_message" => {
            if let Some(text) = agent_message_text(item.content.as_ref()) {
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(ChatContent::Text(text)),
                    ..empty_message()
                });
                report.degraded(
                    Side::Source,
                    format!("/input/{index}/type"),
                    "agent_message is emitted as a user message",
                );
            }
        }
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
            let content = responses_content_to_chat(
                item.content.as_ref(),
                role == "assistant",
                report,
                plan,
                &format!("/input/{index}/content"),
            )?;
            let empty_content = match &content {
                ChatContent::Text(text) => text.is_empty(),
                ChatContent::Parts(parts) => parts.is_empty(),
            };
            if item.role.as_deref() == Some("agent") && empty_content {
                return Ok(());
            }
            messages.push(ChatMessage {
                role: role.to_string(),
                content: Some(content),
                ..empty_message()
            });
        }
        "reasoning" => {
            let summary = item
                .summary
                .as_ref()
                .map(|summary| {
                    summary
                        .iter()
                        .map(|part| part.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .filter(|text| !text.is_empty());
            let decoded = item.encrypted_content.as_deref().and_then(decode_thinking);
            let reasoning = decoded
                .as_ref()
                .and_then(|payload| payload.get("thinking"))
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
                .map(ToString::to_string)
                .or(summary);
            if let Some(reasoning) = reasoning {
                if let Some(last) = messages.last_mut()
                    && last.role == "assistant"
                {
                    last.reasoning_content = Some(reasoning);
                    return Ok(());
                }
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    reasoning_content: Some(reasoning),
                    ..empty_message()
                });
            }
            if item.encrypted_content.is_some() && decoded.is_none() {
                report.degraded(
                    Side::Source,
                    format!("/input/{index}/encrypted_content"),
                    "Responses encrypted reasoning is not an Anthropic thinking envelope and is dropped",
                );
            }
        }
        "tool_search_call" => {
            let arguments = match item.arguments.as_ref() {
                Some(Value::String(text)) => text.clone(),
                Some(value) => serde_json::to_string(value)?,
                None => "{}".to_string(),
            };
            let call = ChatToolCall {
                index: None,
                id: item.call_id.clone(),
                type_: Some("function".to_string()),
                function: ChatFunctionCall {
                    name: Some("tool_search".to_string()),
                    arguments,
                    extra: Map::new(),
                },
                extra: Map::new(),
            };
            if let Some(last) = messages.last_mut()
                && last.role == "assistant"
            {
                last.tool_calls.push(call);
                return Ok(());
            }
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: vec![call],
                ..empty_message()
            });
        }
        "function_call" => {
            let arguments = match item.arguments.as_ref() {
                Some(Value::String(text)) => text.clone(),
                Some(value) => serde_json::to_string(value)?,
                None => String::new(),
            };
            let name = match (
                item.extra.get("namespace").and_then(Value::as_str),
                item.name.as_deref(),
            ) {
                (Some(namespace), Some(name)) if !namespace.is_empty() => {
                    Some(flatten_namespace_name(namespace, name))
                }
                _ => item.name.clone(),
            };
            let call = ChatToolCall {
                index: None,
                id: item.call_id.clone(),
                type_: Some("function".to_string()),
                function: ChatFunctionCall {
                    name,
                    arguments,
                    extra: Map::new(),
                },
                extra: Map::new(),
            };
            if let Some(last) = messages.last_mut()
                && last.role == "assistant"
            {
                last.tool_calls.push(call);
                return Ok(());
            }
            messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: vec![call],
                ..empty_message()
            });
        }
        "custom_tool_call" => {
            let input = match item.input.as_ref() {
                Some(Value::String(text)) => serde_json::json!({"input": text}),
                Some(value) => serde_json::json!({"input": value}),
                None => serde_json::json!({"input": ""}),
            };
            let call = ChatToolCall {
                index: None,
                id: item.call_id.clone(),
                type_: Some("function".to_string()),
                function: ChatFunctionCall {
                    name: item.name.clone(),
                    arguments: serde_json::to_string(&input)?,
                    extra: Map::new(),
                },
                extra: Map::new(),
            };
            if let Some(last) = messages.last_mut()
                && last.role == "assistant"
            {
                last.tool_calls.push(call);
            } else {
                messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: None,
                    tool_calls: vec![call],
                    ..empty_message()
                });
            }
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
            messages.push(ChatMessage {
                role: "tool".to_string(),
                content: Some(ChatContent::Text(content)),
                tool_call_id: item
                    .call_id
                    .clone()
                    .or_else(|| Some("tool_search".to_string())),
                ..empty_message()
            });
        }
        "function_call_output" => {
            let content = match item.output.as_ref() {
                Some(ResponsesToolOutput::Text(text)) => text.clone(),
                Some(ResponsesToolOutput::Parts(parts)) => parts
                    .iter()
                    .filter_map(|part| part.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n"),
                Some(ResponsesToolOutput::Other(value)) => value.to_string(),
                None => String::new(),
            };
            messages.push(ChatMessage {
                role: "tool".to_string(),
                content: Some(ChatContent::Text(content)),
                tool_call_id: item.call_id.clone(),
                ..empty_message()
            });
        }
        other => {
            report.unsupported(
                plan.policy,
                Side::Source,
                format!("/input/{index}/type"),
                format!("Responses input item {other} has no Chat equivalent"),
            )?;
        }
    }
    Ok(())
}

fn agent_message_text(content: Option<&ResponsesContent>) -> Option<String> {
    match content? {
        ResponsesContent::Text(text) => (!text.is_empty()).then(|| text.clone()),
        ResponsesContent::Parts(parts) => {
            let mut text = String::new();
            for part in parts {
                match part.type_.as_str() {
                    "input_text" | "output_text" | "text" => {
                        if let Some(value) = part.text.as_deref() {
                            text.push_str(value);
                        }
                    }
                    "encrypted_content" => {
                        if let Some(value) =
                            part.extra.get("encrypted_content").and_then(Value::as_str)
                        {
                            text.push_str(value);
                        }
                    }
                    _ => {}
                }
            }
            (!text.is_empty()).then_some(text)
        }
    }
}

fn responses_content_to_chat(
    content: Option<&ResponsesContent>,
    _assistant: bool,
    report: &mut Report,
    plan: &Plan,
    pointer: &str,
) -> Result<ChatContent, Error> {
    match content {
        None => Ok(ChatContent::Text(String::new())),
        Some(ResponsesContent::Text(text)) => Ok(ChatContent::Text(text.clone())),
        Some(ResponsesContent::Parts(parts)) => {
            let mut out = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                if !part.annotations.is_empty() {
                    report.degraded(
                        Side::Source,
                        format!("{pointer}/{index}/annotations"),
                        "Chat input content has no annotations field",
                    );
                }
                if part.logprobs.is_some() {
                    report.degraded(
                        Side::Source,
                        format!("{pointer}/{index}/logprobs"),
                        "Chat input content has no logprobs field",
                    );
                }
                if part.prompt_cache_breakpoint.is_some() {
                    report.degraded(
                        Side::Source,
                        format!("{pointer}/{index}/prompt_cache_breakpoint"),
                        "Chat input content has no prompt_cache_breakpoint field",
                    );
                }
                match part.type_.as_str() {
                    "input_text" | "output_text" | "text" => out.push(ChatContentPart {
                        type_: "text".to_string(),
                        text: part.text.clone(),
                        ..empty_part()
                    }),
                    "input_image" => {
                        if let Some(image) = part.image_url.as_ref() {
                            let url = match image {
                                ResponsesImageUrl::Url(url) => url.clone(),
                                ResponsesImageUrl::Object(object) => object.url.clone(),
                            };
                            let detail = part.detail.clone().or_else(|| match image {
                                ResponsesImageUrl::Object(object) => object.detail.clone(),
                                ResponsesImageUrl::Url(_) => None,
                            });
                            out.push(ChatContentPart {
                                type_: "image_url".to_string(),
                                image_url: Some(ChatImageUrl {
                                    url,
                                    detail,
                                    extra: Map::new(),
                                }),
                                ..empty_part()
                            });
                        }
                    }
                    "input_file" => out.push(ChatContentPart {
                        type_: "file".to_string(),
                        file: Some(ChatFile {
                            filename: part.filename.clone(),
                            file_data: part.file_data.clone(),
                            file_id: part.file_id.clone(),
                            extra: Map::new(),
                        }),
                        ..empty_part()
                    }),
                    "input_audio" => {
                        if let Some(audio) = part.extra.get("input_audio")
                            && let Ok(audio) = serde_json::from_value::<
                                crate::wire::openai_chat::ChatInputAudio,
                            >(audio.clone())
                        {
                            if !valid_chat_audio(&audio) {
                                report.unsupported(
                                    plan.policy,
                                    Side::Source,
                                    format!("{pointer}/{index}"),
                                    "Responses input_audio needs base64 data and wav/mp3 format",
                                )?;
                                continue;
                            }
                            out.push(ChatContentPart {
                                type_: "input_audio".to_string(),
                                input_audio: Some(audio),
                                ..empty_part()
                            });
                            report.degraded(
                                Side::Source,
                                format!("{pointer}/{index}"),
                                "Responses input_audio is restored from the extension part",
                            );
                        }
                    }
                    "refusal" => out.push(ChatContentPart {
                        type_: "text".to_string(),
                        text: part.refusal.clone(),
                        ..empty_part()
                    }),
                    other => report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!("{pointer}/{index}/type"),
                        format!("Responses content part {other} has no Chat equivalent"),
                    )?,
                }
            }
            if out.len() == 1 && out[0].type_ == "text" {
                Ok(ChatContent::Text(out[0].text.clone().unwrap_or_default()))
            } else {
                Ok(ChatContent::Parts(out))
            }
        }
    }
}

fn decode_thinking(encrypted: &str) -> Option<Value> {
    if let Some(json) = encrypted.strip_prefix("anthropic-thinking-v1:") {
        return serde_json::from_str(json).ok();
    }
    if encrypted.is_empty() {
        None
    } else {
        Some(serde_json::json!({"signature": encrypted}))
    }
}

fn promote_tool_search_discoveries(tools: &mut Vec<ResponsesTool>, input: Option<&ResponsesInput>) {
    if !tools.iter().any(|tool| tool.type_ == "tool_search") {
        return;
    }
    let Some(ResponsesInput::Items(items)) = input else {
        return;
    };
    let mut declared: BTreeSet<String> =
        tools.iter().filter_map(|tool| tool.name.clone()).collect();
    for item in items {
        if item.type_.as_deref() != Some("tool_search_output") {
            continue;
        }
        let Some(discoveries) = item.extra.get("tools").and_then(Value::as_array) else {
            continue;
        };
        for discovery in discoveries {
            let Some(object) = discovery.as_object() else {
                continue;
            };
            let type_ = object.get("type").and_then(Value::as_str).unwrap_or("");
            let name = object.get("name").and_then(Value::as_str).unwrap_or("");
            if type_ == "namespace" {
                let Some(children) = object.get("tools").and_then(Value::as_array) else {
                    continue;
                };
                for child in children {
                    let Some(child) = child.as_object() else {
                        continue;
                    };
                    let child_type = child.get("type").and_then(Value::as_str).unwrap_or("");
                    let child_name = child.get("name").and_then(Value::as_str).unwrap_or("");
                    if !name.is_empty() && !child_name.is_empty() {
                        push_discovered_tool(
                            tools,
                            &mut declared,
                            child_type,
                            &flatten_namespace_name(name, child_name),
                            child,
                        );
                    }
                }
                continue;
            }
            if !name.is_empty() {
                push_discovered_tool(tools, &mut declared, type_, name, object);
            }
        }
    }
}

fn push_discovered_tool(
    tools: &mut Vec<ResponsesTool>,
    declared: &mut BTreeSet<String>,
    type_: &str,
    name: &str,
    object: &serde_json::Map<String, Value>,
) {
    if (type_ != "function" && type_ != "custom") || name.is_empty() || declared.contains(name) {
        return;
    }
    declared.insert(name.to_string());
    tools.push(ResponsesTool {
        type_: type_.to_string(),
        name: Some(name.to_string()),
        description: object
            .get("description")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        parameters: object.get("parameters").cloned(),
        strict: object.get("strict").and_then(Value::as_bool),
        format: object.get("format").cloned(),
        tools: Vec::new(),
        children: Vec::new(),
        extra: Map::new(),
    });
}

fn promote_additional_tools(tools: &mut Vec<ResponsesTool>, input: Option<&ResponsesInput>) {
    let Some(ResponsesInput::Items(items)) = input else {
        return;
    };
    for item in items {
        if item.type_.as_deref() != Some("additional_tools") {
            continue;
        }
        let Some(additional) = item.extra.get("tools").and_then(Value::as_array) else {
            continue;
        };
        for tool in additional {
            if let Ok(tool) = serde_json::from_value::<ResponsesTool>(tool.clone()) {
                tools.push(tool);
            }
        }
    }
}

fn flatten_additional_tools(tools: &mut Vec<ResponsesTool>) {
    let mut index = 0;
    while index < tools.len() {
        if tools[index].type_ == "additional_tools" {
            let mut additional = tools.remove(index);
            let children = if additional.tools.is_empty() {
                std::mem::take(&mut additional.children)
            } else {
                std::mem::take(&mut additional.tools)
            };
            for child in children.into_iter().rev() {
                tools.insert(index, child);
            }
        } else {
            index += 1;
        }
    }
}

fn convert_tools(
    tools: &[ResponsesTool],
    report: &mut Report,
    plan: &Plan,
) -> Result<Vec<ChatTool>, Error> {
    let mut top_level = BTreeSet::new();
    for tool in tools {
        if matches!(tool.type_.as_str(), "function" | "custom")
            && let Some(name) = tool.name.as_deref()
            && !top_level.insert(name.to_string())
        {
            return Err(Error::Invalid(format!(
                "duplicate top-level executable tool name {name}"
            )));
        }
    }

    let mut flat_owner: HashMap<String, (String, String)> = HashMap::new();
    let mut tool_search_declared = false;
    let mut out = Vec::new();
    for (index, tool) in tools.iter().enumerate() {
        match tool.type_.as_str() {
            "function" => {
                let Some(name) = tool.name.as_ref() else {
                    report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!("/tools/{index}/name"),
                        "Responses function tool is missing name",
                    )?;
                    continue;
                };
                out.push(ChatTool {
                    type_: "function".to_string(),
                    function: Some(ChatFunction {
                        name: name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.parameters.clone(),
                        strict: tool.strict,
                        extra: Map::new(),
                    }),
                    extra: Map::new(),
                });
            }
            "custom" => {
                let Some(name) = tool.name.as_ref() else {
                    continue;
                };
                out.push(ChatTool {
                    type_: "function".to_string(),
                    function: Some(ChatFunction {
                        name: name.clone(),
                        description: tool.description.clone(),
                        parameters: Some(custom_tool_input_schema()),
                        strict: None,
                        extra: Map::new(),
                    }),
                    extra: Map::new(),
                });
            }
            "tool_search" => {
                if top_level.contains("tool_search") {
                    return Err(Error::Invalid(
                        "built-in tool_search conflicts with a declared tool named tool_search"
                            .to_string(),
                    ));
                }
                if tool_search_declared {
                    continue;
                }
                tool_search_declared = true;
                out.push(ChatTool {
                    type_: "function".to_string(),
                    function: Some(ChatFunction {
                        name: "tool_search".to_string(),
                        description: Some(
                            "Search and load Codex tools, plugins, connectors, and MCP namespaces for the current task."
                                .to_string(),
                        ),
                        parameters: Some(json!({
                            "type": "object",
                            "properties": {
                                "query": {"type": "string", "description": "Search query for tools or connectors to load."},
                                "limit": {"type": "integer", "description": "Maximum number of tool groups to return."}
                            },
                            "required": ["query"]
                        })),
                        strict: None,
                        extra: Map::new(),
                    }),
                    extra: Map::new(),
                });
            }
            "namespace" => {
                let Some(namespace) = tool.name.as_deref() else {
                    continue;
                };
                let children = if tool.tools.is_empty() {
                    &tool.children
                } else {
                    &tool.tools
                };
                for child in children {
                    if child.type_ != "function" {
                        continue;
                    }
                    let Some(child_name) = child.name.as_deref() else {
                        continue;
                    };
                    let flat = flatten_namespace_name(namespace, child_name);
                    if top_level.contains(&flat) {
                        return Err(Error::Invalid(format!(
                            "namespace tool {namespace}/{child_name} flattens to {flat} which conflicts with a top-level tool"
                        )));
                    }
                    if let Some((previous_namespace, previous_name)) = flat_owner.get(&flat) {
                        if previous_namespace != namespace || previous_name != child_name {
                            return Err(Error::Invalid(format!(
                                "namespace tools {previous_namespace}/{previous_name} and {namespace}/{child_name} both flatten to {flat}"
                            )));
                        }
                        continue;
                    }
                    flat_owner.insert(
                        flat.clone(),
                        (namespace.to_string(), child_name.to_string()),
                    );
                    out.push(ChatTool {
                        type_: "function".to_string(),
                        function: Some(ChatFunction {
                            name: flat,
                            description: child.description.clone(),
                            parameters: child.parameters.clone(),
                            strict: child.strict,
                            extra: Map::new(),
                        }),
                        extra: Map::new(),
                    });
                }
            }
            "x_search" => out.push(ChatTool {
                type_: "x_search".to_string(),
                function: None,
                extra: tool.extra.clone(),
            }),
            "web_search" | "code_execution" | "file_search" => {
                report.degraded(
                    Side::Source,
                    format!("/tools/{index}/type"),
                    format!(
                        "Responses tool {} has no Chat equivalent; dropped",
                        tool.type_
                    ),
                );
            }
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("/tools/{index}/type"),
                    format!("Responses tool {other} has no Chat equivalent"),
                )?;
            }
        }
    }
    Ok(out)
}

fn convert_tool_choice(
    choice: Option<&Value>,
    declared: &[String],
    report: &mut Report,
    plan: &Plan,
) -> Result<Option<ChatToolChoice>, Error> {
    let Some(choice) = choice else {
        return Ok(None);
    };
    let mapped = match choice {
        Value::String(mode) => match mode.as_str() {
            "auto" | "none" | "required" => Some(ChatToolChoice::Mode(mode.clone())),
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    "/tool_choice",
                    format!("unknown Responses tool_choice mode: {other}"),
                )?;
                None
            }
        },
        Value::Object(object) => {
            let type_ = object.get("type").and_then(Value::as_str).unwrap_or("");
            if type_ == "allowed_tools" {
                return Ok(Some(ChatToolChoice::Object(object.clone())));
            }
            let name = match type_ {
                "x_search" => "x_search".to_string(),
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
                report.degraded(
                    Side::Source,
                    "/tool_choice",
                    "Responses tool_choice object has no Chat equivalent",
                );
                None
            } else if !declared.iter().any(|declared| declared == &name) {
                report.degraded(
                    Side::Source,
                    "/tool_choice",
                    format!("tool_choice references undeclared tool {name}; dropped"),
                );
                None
            } else if type_ == "x_search" {
                Some(ChatToolChoice::Mode("x_search".to_string()))
            } else {
                let mut out = Map::new();
                out.insert("type".to_string(), json!("function"));
                out.insert("function".to_string(), json!({"name": name}));
                Some(ChatToolChoice::Object(out))
            }
        }
        _ => None,
    };
    Ok(mapped)
}
