//! Chat request -> Responses request。

use std::collections::{HashMap, HashSet, VecDeque};

use serde_json::{Map, Value, json};

use crate::common::model::{MIN_OUTPUT_TOKENS, is_reasoning_model};
use crate::registry::Plan;
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::openai_chat::{ChatContent, ChatMessage, ChatRequest, ChatToolChoice};
use crate::wire::openai_responses::{
    ResponsesContent, ResponsesContentPart, ResponsesImageUrl, ResponsesInput, ResponsesInputItem,
    ResponsesReasoning, ResponsesRequest, ResponsesText, ResponsesTool, ResponsesToolOutput,
};

pub(super) fn convert(req: ChatRequest, plan: &Plan) -> Result<(ResponsesRequest, Report), Error> {
    let mut report = Report::default();

    let instructions = req.instructions.clone().filter(|text| !text.is_empty());

    let legacy_ids = assign_legacy_function_ids(&req.messages);
    for key in req.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Chat request field is not mapped to Responses",
        );
    }

    let mut input = Vec::new();
    for (index, message) in req.messages.iter().enumerate() {
        if message.name.is_some() {
            report.degraded(
                Side::Source,
                format!("/messages/{index}/name"),
                "Responses has no message name field",
            );
        }
        for key in message.extra.keys() {
            report.warn(
                WarningCode::UnknownField,
                Side::Source,
                format!("/messages/{index}/{key}"),
                "unknown Chat message field is not mapped to Responses",
            );
        }
        match message.role.as_str() {
            "system" | "developer" => {
                let content = chat_content_to_responses(
                    message.content.as_ref(),
                    false,
                    &mut report,
                    plan,
                    &format!("/messages/{index}/content"),
                )?;
                input.push(ResponsesInputItem {
                    type_: Some("message".to_string()),
                    role: Some(message.role.clone()),
                    content: Some(content),
                    ..empty_item()
                });
            }
            "user" => {
                let mut content = chat_content_to_responses(
                    message.content.as_ref(),
                    false,
                    &mut report,
                    plan,
                    &format!("/messages/{index}/content"),
                )?;
                append_refusal(
                    &mut content,
                    message.refusal.as_deref(),
                    "input_text",
                    index,
                    &mut report,
                );
                input.push(ResponsesInputItem {
                    type_: Some("message".to_string()),
                    role: Some("user".to_string()),
                    content: Some(content),
                    ..empty_item()
                });
            }
            "assistant" => {
                let reasoning = message
                    .reasoning_content
                    .as_deref()
                    .or(message.reasoning.as_deref())
                    .filter(|text| !text.is_empty());
                let mut content = chat_content_to_responses(
                    message.content.as_ref(),
                    true,
                    &mut report,
                    plan,
                    &format!("/messages/{index}/content"),
                )?;
                if let Some(reasoning) = reasoning {
                    prepend_inline_thinking(&mut content, reasoning);
                    report.degraded(
                        Side::Source,
                        format!("/messages/{index}/reasoning_content"),
                        "Chat reasoning is replayed inline as <thinking>",
                    );
                }
                append_refusal(
                    &mut content,
                    message.refusal.as_deref(),
                    "output_text",
                    index,
                    &mut report,
                );
                if !is_empty_content(&content) {
                    input.push(ResponsesInputItem {
                        type_: Some("message".to_string()),
                        role: Some("assistant".to_string()),
                        content: Some(content),
                        ..empty_item()
                    });
                }
                if let Some(call) = message.function_call.as_ref()
                    && message.tool_calls.is_empty()
                {
                    input.push(ResponsesInputItem {
                        type_: Some("function_call".to_string()),
                        call_id: legacy_ids[index]
                            .clone()
                            .or_else(|| message.tool_call_id.clone())
                            .or_else(|| call.name.clone()),
                        name: call.name.clone(),
                        arguments: Some(Value::String(call.arguments.clone())),
                        ..empty_item()
                    });
                    report.degraded(
                        Side::Target,
                        format!("/messages/{index}/function_call"),
                        "legacy Chat function_call is emitted as Responses function_call",
                    );
                } else if message.tool_call_id.is_some() && message.function_call.is_none() {
                    report.degraded(
                        Side::Source,
                        format!("/messages/{index}/tool_call_id"),
                        "assistant tool_call_id has no Responses equivalent",
                    );
                }
                for (tool_index, call) in message.tool_calls.iter().enumerate() {
                    if call.type_.as_deref().is_some_and(|kind| kind != "function") {
                        report.degraded(
                            Side::Source,
                            format!("/messages/{index}/tool_calls/{tool_index}/type"),
                            "Chat tool call type other than function has no Responses equivalent",
                        );
                    }
                    input.push(ResponsesInputItem {
                        type_: Some("function_call".to_string()),
                        call_id: call.id.clone(),
                        name: call.function.name.clone(),
                        arguments: Some(Value::String(call.function.arguments.clone())),
                        ..empty_item()
                    });
                    if call.id.is_none() {
                        report.degraded(
                            Side::Target,
                            format!("/messages/{index}/tool_calls/{tool_index}/id"),
                            "Chat tool call has no id; Responses function_call has no call_id",
                        );
                    }
                }
            }
            "tool" | "function" => input.push(ResponsesInputItem {
                type_: Some("function_call_output".to_string()),
                call_id: legacy_ids[index]
                    .clone()
                    .or_else(|| message.tool_call_id.clone())
                    .or_else(|| message.name.clone()),
                output: Some(match message.content.as_ref() {
                    Some(ChatContent::Text(text)) => ResponsesToolOutput::Text(text.clone()),
                    Some(ChatContent::Parts(parts)) => ResponsesToolOutput::Text(
                        parts
                            .iter()
                            .filter_map(|part| part.text.as_deref())
                            .collect::<String>(),
                    ),
                    None => ResponsesToolOutput::Text(String::new()),
                }),
                ..empty_item()
            }),
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("/messages/{index}/role"),
                    format!("unknown Chat role: {other}"),
                )?;
            }
        }
    }

    if input.is_empty() {
        return Err(Error::Invalid(
            "Chat request has no convertible input".to_string(),
        ));
    }

    let mut tools = Vec::new();
    for (index, tool) in req.tools.iter().enumerate() {
        match tool.type_.to_ascii_lowercase().as_str() {
            "x_search" => tools.push(ResponsesTool {
                type_: "x_search".to_string(),
                name: None,
                description: None,
                parameters: None,
                strict: None,
                format: None,
                tools: Vec::new(),
                children: Vec::new(),
                extra: tool.extra.clone(),
            }),
            "web_search" | "code_execution" => tools.push(ResponsesTool {
                type_: tool.type_.clone(),
                name: None,
                description: None,
                parameters: None,
                strict: None,
                format: None,
                tools: Vec::new(),
                children: Vec::new(),
                extra: tool.extra.clone(),
            }),
            "function" => {
                let Some(function) = tool.function.as_ref() else {
                    continue;
                };
                tools.push(ResponsesTool {
                    type_: "function".to_string(),
                    name: Some(function.name.clone()),
                    description: function.description.clone(),
                    parameters: function.parameters.clone(),
                    strict: function.strict.or(Some(false)),
                    format: None,
                    tools: Vec::new(),
                    children: Vec::new(),
                    extra: Map::new(),
                });
            }
            "custom" => {
                let Some(function) = tool.function.as_ref() else {
                    continue;
                };
                tools.push(ResponsesTool {
                    type_: "custom".to_string(),
                    name: Some(function.name.clone()),
                    description: function.description.clone(),
                    parameters: function.parameters.clone(),
                    strict: None,
                    format: tool.extra.get("format").cloned(),
                    tools: Vec::new(),
                    children: Vec::new(),
                    extra: Map::new(),
                });
            }
            other => report.unsupported(
                plan.policy,
                Side::Source,
                format!("/tools/{index}/type"),
                format!("Chat tool type {other} has no Responses equivalent"),
            )?,
        }
    }
    for function in &req.functions {
        tools.push(ResponsesTool {
            type_: "function".to_string(),
            name: Some(function.name.clone()),
            description: function.description.clone(),
            parameters: function.parameters.clone(),
            strict: function.strict.or(Some(false)),
            ..empty_tool()
        });
        report.degraded(
            Side::Source,
            "/functions",
            "legacy Chat functions are emitted as Responses tools",
        );
    }

    let tool_choice = if req.tool_choice.is_some() {
        convert_tool_choice(req.tool_choice.as_ref(), &mut report, plan)?
    } else {
        req.function_call
            .as_ref()
            .and_then(convert_legacy_function_call)
    };

    let text = req.response_format.as_ref().map(|format| ResponsesText {
        format: Some(match format.json_schema.as_ref() {
            Some(schema) if schema.get("schema").is_some() => {
                let mut value = json!({
                    "type": "json_schema",
                    "name": schema.get("name").cloned().unwrap_or_else(|| json!("response")),
                    "schema": schema.get("schema").cloned().unwrap_or_else(|| json!({}))
                });
                if let Some(strict) = schema.get("strict") {
                    value["strict"] = strict.clone();
                }
                value
            }
            Some(schema) => json!({
                "type": "json_schema",
                "name": "response",
                "schema": schema
            }),
            None => json!({"type": format.type_}),
        }),
        verbosity: None,
        extra: Map::new(),
    });

    let reasoning = req
        .reasoning_effort
        .as_ref()
        .map(|effort| ResponsesReasoning {
            effort: Some(effort.clone()),
            summary: Some("auto".to_string()),
            max_tokens: None,
            extra: Map::new(),
        });

    if req.stop.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/stop",
            "Responses has no stop parameter",
        )?;
    }
    if req.n.is_some() {
        report.unsupported(plan.policy, Side::Source, "/n", "Responses has no n")?;
    }
    if req.seed.is_some() {
        report.unsupported(plan.policy, Side::Source, "/seed", "Responses has no seed")?;
    }
    if req.presence_penalty.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/presence_penalty",
            "Responses has no presence_penalty",
        )?;
    }
    if req.frequency_penalty.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/frequency_penalty",
            "Responses has no frequency_penalty",
        )?;
    }
    if req.logprobs.is_some() || req.top_logprobs.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/logprobs",
            "Responses has no logprobs",
        )?;
    }
    if req.stream_options.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/stream_options",
            "Responses has no stream_options",
        )?;
    }
    if req.user.is_some() {
        report.degraded(
            Side::Source,
            "/user",
            "Chat user is dropped; Responses has no equivalent",
        );
    }

    let mut include = Vec::new();
    if reasoning.is_some() {
        include.push("reasoning.encrypted_content".to_string());
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

    let max_output_tokens = req.max_completion_tokens.or(req.max_tokens).map(|value| {
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
        instructions,
        input: Some(ResponsesInput::Items(input)),
        max_output_tokens,
        temperature,
        top_p: req.top_p,
        stream: Some(true),
        tools,
        include: vec!["reasoning.encrypted_content".to_string()],
        store: Some(false),
        parallel_tool_calls: req.parallel_tool_calls,
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

fn is_empty_base64_data_uri(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("data:") else {
        return false;
    };
    let Some((_, data)) = rest.split_once(',') else {
        return false;
    };
    data.trim().is_empty()
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

fn empty_tool() -> ResponsesTool {
    ResponsesTool {
        type_: "function".to_string(),
        name: None,
        description: None,
        parameters: None,
        strict: None,
        format: None,
        tools: Vec::new(),
        children: Vec::new(),
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

fn assign_legacy_function_ids(messages: &[ChatMessage]) -> Vec<Option<String>> {
    let mut used = HashSet::new();
    for message in messages {
        for call in &message.tool_calls {
            if let Some(id) = call.id.as_ref() {
                used.insert(id.clone());
            }
        }
        if let Some(id) = message.tool_call_id.as_ref() {
            used.insert(id.clone());
        }
    }
    let mut pending: HashMap<String, VecDeque<String>> = HashMap::new();
    let mut assigned = vec![None; messages.len()];
    let mut counter = 0;
    for (index, message) in messages.iter().enumerate() {
        if message.role == "assistant"
            && message.function_call.is_some()
            && message.tool_calls.is_empty()
        {
            let name = message
                .function_call
                .as_ref()
                .and_then(|call| call.name.clone())
                .unwrap_or_default();
            loop {
                counter += 1;
                let id = format!("call_legacy_{counter}");
                if !used.contains(&id) {
                    used.insert(id.clone());
                    pending
                        .entry(name.clone())
                        .or_default()
                        .push_back(id.clone());
                    assigned[index] = Some(id);
                    break;
                }
            }
        } else if message.role == "function" && message.tool_call_id.is_none() {
            let name = message.name.clone().unwrap_or_default();
            if let Some(queue) = pending.get_mut(&name)
                && !queue.is_empty()
            {
                assigned[index] = queue.pop_front();
            }
            if assigned[index].is_none() {
                assigned[index] = Some(name);
            }
        }
    }
    assigned
}

fn convert_legacy_function_call(raw: &Value) -> Option<Value> {
    match raw {
        Value::String(mode) => Some(Value::String(mode.clone())),
        Value::Object(object) => object
            .get("name")
            .and_then(Value::as_str)
            .map(|name| json!({"type": "function", "name": name})),
        _ => None,
    }
}

fn prepend_inline_thinking(content: &mut ResponsesContent, reasoning: &str) {
    let thinking = format!("<thinking>{reasoning}</thinking>");
    match content {
        ResponsesContent::Text(text) => {
            let existing = std::mem::take(text);
            let combined = if existing.is_empty() {
                thinking
            } else {
                format!("{thinking}\n{existing}")
            };
            *content = ResponsesContent::Parts(vec![ResponsesContentPart {
                type_: "output_text".to_string(),
                text: Some(combined),
                ..empty_part()
            }]);
        }
        ResponsesContent::Parts(parts) => {
            if let Some(part) = parts.iter_mut().find(|part| part.text.is_some()) {
                let existing = part.text.take().unwrap_or_default();
                part.text = Some(if existing.is_empty() {
                    thinking
                } else {
                    format!("{thinking}\n{existing}")
                });
            } else {
                parts.insert(
                    0,
                    ResponsesContentPart {
                        type_: "output_text".to_string(),
                        text: Some(thinking),
                        ..empty_part()
                    },
                );
            }
        }
    }
}

fn is_empty_content(content: &ResponsesContent) -> bool {
    match content {
        ResponsesContent::Text(text) => text.is_empty(),
        ResponsesContent::Parts(parts) => parts.iter().all(|part| {
            part.text.as_deref().is_none_or(str::is_empty)
                && part.image_url.is_none()
                && part.file_data.is_none()
                && part.file_id.is_none()
        }),
    }
}

fn append_refusal(
    content: &mut ResponsesContent,
    refusal: Option<&str>,
    text_type: &str,
    message_index: usize,
    report: &mut Report,
) {
    let Some(refusal) = refusal.filter(|text| !text.is_empty()) else {
        return;
    };
    let part = ResponsesContentPart {
        type_: text_type.to_string(),
        text: Some(refusal.to_string()),
        ..empty_part()
    };
    match content {
        ResponsesContent::Text(text) => {
            *content = ResponsesContent::Parts(vec![
                ResponsesContentPart {
                    type_: text_type.to_string(),
                    text: Some(std::mem::take(text)),
                    ..empty_part()
                },
                part,
            ]);
        }
        ResponsesContent::Parts(parts) => parts.push(part),
    }
    report.degraded(
        Side::Target,
        format!("/messages/{message_index}/refusal"),
        "Chat refusal is emitted as a Responses text part",
    );
}

fn chat_content_to_responses(
    content: Option<&ChatContent>,
    assistant: bool,
    report: &mut Report,
    plan: &Plan,
    pointer: &str,
) -> Result<ResponsesContent, Error> {
    let text_type = if assistant {
        "output_text"
    } else {
        "input_text"
    };
    match content {
        None => Ok(ResponsesContent::Text(String::new())),
        Some(ChatContent::Text(text)) => Ok(ResponsesContent::Text(text.clone())),
        Some(ChatContent::Parts(parts)) => {
            let mut out = Vec::new();
            let mut text = String::new();
            let mut has_text = false;
            let mut after_thinking = false;
            for (index, part) in parts.iter().enumerate() {
                match part.type_.as_str() {
                    "text" => {
                        let fragment = part.text.as_deref().unwrap_or("");
                        if !fragment.is_empty() {
                            if after_thinking {
                                text.push('\n');
                                after_thinking = false;
                            }
                            text.push_str(fragment);
                            has_text = true;
                        }
                    }
                    "image_url" => {
                        if let Some(image) = part.image_url.as_ref()
                            && !is_empty_base64_data_uri(&image.url)
                            && !image.url.trim().is_empty()
                        {
                            out.push(ResponsesContentPart {
                                type_: "input_image".to_string(),
                                image_url: Some(ResponsesImageUrl::Url(image.url.clone())),
                                detail: image.detail.clone(),
                                ..empty_part()
                            });
                        }
                    }
                    "file" => {
                        if let Some(file) = part.file.as_ref()
                            && (file.file_data.is_some() || file.file_id.is_some())
                        {
                            out.push(ResponsesContentPart {
                                type_: "input_file".to_string(),
                                filename: file.filename.clone(),
                                file_data: file.file_data.clone(),
                                file_id: file.file_id.clone(),
                                ..empty_part()
                            });
                        }
                    }
                    "input_audio" => {
                        if let Some(audio) = part.input_audio.as_ref() {
                            if !valid_chat_audio(audio) {
                                report.unsupported(
                                    plan.policy,
                                    Side::Source,
                                    format!("{pointer}/{index}"),
                                    "Chat input_audio needs base64 data and wav/mp3 format",
                                )?;
                                continue;
                            }
                            let mut extra = Map::new();
                            extra.insert("input_audio".to_string(), serde_json::to_value(audio)?);
                            out.push(ResponsesContentPart {
                                type_: "input_audio".to_string(),
                                extra,
                                ..empty_part()
                            });
                            report.degraded(
                                Side::Target,
                                format!("{pointer}/{index}"),
                                "Chat input_audio is carried as a Responses extension part",
                            );
                        }
                    }
                    "thinking" => {
                        let fragment = part
                            .text
                            .clone()
                            .or_else(|| {
                                part.extra
                                    .get("thinking")
                                    .and_then(Value::as_str)
                                    .map(ToString::to_string)
                            })
                            .unwrap_or_default();
                        if !fragment.is_empty() {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str("<thinking>");
                            text.push_str(&fragment);
                            text.push_str("</thinking>");
                            has_text = true;
                            after_thinking = true;
                        }
                    }
                    "refusal" => {
                        if let Some(fragment) =
                            part.refusal.as_deref().filter(|text| !text.is_empty())
                        {
                            if after_thinking {
                                text.push('\n');
                                after_thinking = false;
                            }
                            text.push_str(fragment);
                            has_text = true;
                        }
                        report.degraded(
                            Side::Target,
                            format!("{pointer}/{index}"),
                            "Chat refusal is emitted as text",
                        );
                    }
                    other => {
                        report.unsupported(
                            plan.policy,
                            Side::Source,
                            format!("{pointer}/{index}/type"),
                            format!("unknown Chat content type: {other}"),
                        )?;
                    }
                }
            }
            if has_text {
                out.insert(
                    0,
                    ResponsesContentPart {
                        type_: text_type.to_string(),
                        text: Some(text),
                        ..empty_part()
                    },
                );
            }
            if out.is_empty() {
                Ok(ResponsesContent::Text(String::new()))
            } else {
                Ok(ResponsesContent::Parts(out))
            }
        }
    }
}

fn convert_tool_choice(
    choice: Option<&ChatToolChoice>,
    report: &mut Report,
    plan: &Plan,
) -> Result<Option<Value>, Error> {
    let Some(choice) = choice else {
        return Ok(None);
    };
    match choice {
        ChatToolChoice::Mode(mode) => match mode.as_str() {
            "auto" | "none" | "required" => Ok(Some(json!(mode))),
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    "/tool_choice",
                    format!("unknown Chat tool_choice mode: {other}"),
                )?;
                Ok(None)
            }
        },
        ChatToolChoice::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("allowed_tools") {
                return Ok(Some(Value::Object(object.clone())));
            }
            if object.get("type").and_then(Value::as_str) == Some("function")
                && let Some(name) = object.get("name").and_then(Value::as_str).or_else(|| {
                    object
                        .get("function")
                        .and_then(|value| value.get("name"))
                        .and_then(Value::as_str)
                })
            {
                return Ok(Some(json!({"type": "function", "name": name})));
            }
            report.unsupported(
                plan.policy,
                Side::Source,
                "/tool_choice",
                "Chat tool_choice object has no Responses equivalent",
            )?;
            Ok(None)
        }
    }
}
