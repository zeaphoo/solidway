//! Responses response -> Anthropic response。

use serde_json::{Map, Value, json};

use crate::common::empty;
use crate::common::usage::{positive, saturating_deduct};
use crate::registry::{Plan, Transcoded};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{
    AnthropicContentBlock, AnthropicError, AnthropicErrorBody, AnthropicResponse,
    AnthropicToolResultContent, AnthropicUsage,
};
use crate::wire::openai_responses::{ResponsesErrorEnvelope, ResponsesResponse};

pub(super) fn convert(body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
    if empty::is_empty_body(body) {
        return Ok(Transcoded {
            body: serde_json::to_vec(&empty::anthropic_response())?,
            report: Report::default(),
        });
    }

    let value: Value = serde_json::from_slice(body)?;

    if value.get("error").is_some() {
        let err: ResponsesErrorEnvelope = serde_json::from_value(value)?;
        let mut extra = err.error.extra;
        if let Some(code) = err.error.code {
            extra
                .entry("code".to_string())
                .or_insert(Value::String(code));
        }
        if let Some(param) = err.error.param {
            extra
                .entry("param".to_string())
                .or_insert(Value::String(param));
        }
        return Ok(Transcoded {
            body: serde_json::to_vec(&AnthropicError {
                type_: "error".to_string(),
                error: AnthropicErrorBody {
                    type_: map_error_type(err.error.type_.as_deref()),
                    message: err.error.message,
                    extra,
                },
                request_id: None,
                extra: Map::new(),
            })?,
            report: Report::default(),
        });
    }

    let resp: ResponsesResponse = serde_json::from_value(value)?;
    let mut report = Report::default();
    for key in resp.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Responses field is not mapped to Anthropic",
        );
    }
    for (pointer, present) in [
        ("/instructions", resp.instructions.is_some()),
        ("/metadata", resp.metadata.is_some()),
        ("/parallel_tool_calls", resp.parallel_tool_calls.is_some()),
        ("/previous_response_id", resp.previous_response_id.is_some()),
        ("/reasoning", resp.reasoning.is_some()),
        ("/store", resp.store.is_some()),
        ("/temperature", resp.temperature.is_some()),
        ("/text", resp.text.is_some()),
        ("/tool_choice", resp.tool_choice.is_some()),
        ("/tools", !resp.tools.is_empty()),
        ("/top_p", resp.top_p.is_some()),
        ("/truncation", resp.truncation.is_some()),
    ] {
        if present {
            report.degraded(
                Side::Source,
                pointer,
                "Responses request echo field has no Anthropic response equivalent",
            );
        }
    }

    let mut content = Vec::new();
    let mut has_tool_use = false;

    for (index, item) in resp.output.iter().enumerate() {
        for key in item.extra.keys() {
            report.warn(
                WarningCode::UnknownField,
                Side::Source,
                format!("/output/{index}/{key}"),
                "unknown Responses output field is not mapped to Anthropic",
            );
        }
        if item.namespace.is_some() {
            report.degraded(
                Side::Source,
                format!("/output/{index}/namespace"),
                "Anthropic has no namespace field",
            );
        }
        match item.type_.as_str() {
            "reasoning" => {
                if item.encrypted_content.is_none() {
                    report.degraded(
                        Side::Source,
                        format!("/output/{index}"),
                        "reasoning item without encrypted_content is dropped",
                    );
                    continue;
                }
                let text = item
                    .summary
                    .iter()
                    .map(|part| part.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let decoded = item.encrypted_content.as_deref().and_then(decode_thinking);
                let (thinking, signature) = match decoded {
                    Some(payload) => (
                        payload
                            .get("thinking")
                            .and_then(Value::as_str)
                            .filter(|text| !text.is_empty())
                            .map(ToString::to_string)
                            .unwrap_or(text),
                        payload
                            .get("signature")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                    ),
                    None => {
                        if item.encrypted_content.is_some() {
                            report.degraded(
                                Side::Target,
                                format!("/output/{index}/encrypted_content"),
                                "Responses encrypted_content is not an Anthropic thinking envelope",
                            );
                        }
                        (text, None)
                    }
                };
                content.push(AnthropicContentBlock {
                    type_: "thinking".to_string(),
                    thinking: Some(thinking),
                    signature,
                    ..empty_block()
                });
            }
            "message" => {
                for (part_index, part) in item.content.iter().enumerate() {
                    if part.logprobs.is_some() {
                        report.degraded(
                            Side::Source,
                            format!("/output/{index}/content/{part_index}/logprobs"),
                            "Anthropic has no logprobs field",
                        );
                    }
                    if part.prompt_cache_breakpoint.is_some() {
                        report.degraded(
                            Side::Source,
                            format!("/output/{index}/content/{part_index}/prompt_cache_breakpoint"),
                            "Anthropic has no prompt_cache_breakpoint field",
                        );
                    }
                    match part.type_.as_str() {
                        "output_text" => {
                            if let Some(text) = part.text.as_deref().filter(|text| !text.is_empty())
                            {
                                content.push(AnthropicContentBlock {
                                    type_: "text".to_string(),
                                    text: Some(text.to_string()),
                                    citations: part.annotations.clone(),
                                    ..empty_block()
                                });
                            }
                        }
                        "refusal" => {
                            if let Some(refusal) =
                                part.refusal.as_deref().filter(|text| !text.is_empty())
                            {
                                content.push(AnthropicContentBlock {
                                    type_: "text".to_string(),
                                    text: Some(refusal.to_string()),
                                    ..empty_block()
                                });
                                report.degraded(
                                    Side::Target,
                                    format!("/output/{index}/content"),
                                    "Responses refusal is emitted as Anthropic text",
                                );
                            }
                        }
                        other => report.unsupported(
                            plan.policy,
                            Side::Target,
                            format!("/output/{index}/content/{other}"),
                            format!("Responses content part {other} has no Anthropic equivalent"),
                        )?,
                    }
                }
            }
            "custom_tool_call" => {
                has_tool_use = true;
                let input = match item.input.as_ref() {
                    Some(Value::String(text)) => json!({"input": text}),
                    Some(value) => json!({"input": value}),
                    None => json!({"input": ""}),
                };
                content.push(AnthropicContentBlock {
                    type_: "tool_use".to_string(),
                    id: item.call_id.clone(),
                    name: item.name.clone(),
                    input: Some(input),
                    ..empty_block()
                });
            }
            "tool_search_call" => {
                has_tool_use = true;
                let input =
                    item.arguments
                        .as_ref()
                        .map(|value| match value {
                            Value::String(text) => serde_json::from_str(text)
                                .unwrap_or_else(|_| json!({"query": text})),
                            other => other.clone(),
                        })
                        .unwrap_or_else(|| json!({}));
                content.push(AnthropicContentBlock {
                    type_: "tool_use".to_string(),
                    id: item.call_id.clone(),
                    name: Some("tool_search".to_string()),
                    input: Some(if input.is_object() { input } else { json!({}) }),
                    ..empty_block()
                });
            }
            "x_search_call" | "web_search_call" => {
                has_tool_use = true;
                let name = item.type_.trim_end_matches("_call").to_string();
                let tool_use_id = responses_server_tool_id(item, index);
                let input = responses_server_tool_input(item);
                let results = responses_server_tool_results(item);
                let citations = item
                    .extra
                    .get("citations")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                content.push(AnthropicContentBlock {
                    type_: "server_tool_use".to_string(),
                    id: Some(tool_use_id.clone()),
                    name: Some(name.clone()),
                    input: Some(input),
                    ..empty_block()
                });
                content.push(AnthropicContentBlock {
                    type_: format!("{name}_tool_result"),
                    tool_use_id: Some(tool_use_id),
                    content: Some(AnthropicToolResultContent::Other(results)),
                    citations,
                    ..empty_block()
                });
            }
            "function_call" => {
                has_tool_use = true;
                let input = item
                    .arguments
                    .as_ref()
                    .map(|value| match value {
                        Value::String(text) => {
                            serde_json::from_str(text).unwrap_or_else(|_| json!({}))
                        }
                        other => other.clone(),
                    })
                    .unwrap_or_else(|| json!({}));
                let input = sanitize_read_input(item.name.as_deref(), input);
                content.push(AnthropicContentBlock {
                    type_: "tool_use".to_string(),
                    id: item.call_id.clone(),
                    name: item.name.clone(),
                    input: Some(if input.is_object() { input } else { json!({}) }),
                    ..empty_block()
                });
            }
            other => {
                if let Some(text) = output_item_text(item) {
                    content.push(AnthropicContentBlock {
                        type_: "text".to_string(),
                        text: Some(text),
                        ..empty_block()
                    });
                    report.degraded(
                        Side::Target,
                        format!("/output/{index}/type"),
                        format!("unknown Responses item {other} is emitted as text"),
                    );
                } else {
                    report.unsupported(
                        plan.policy,
                        Side::Target,
                        format!("/output/{index}/type"),
                        format!("Responses output item {other} has no Anthropic equivalent"),
                    )?;
                }
            }
        }
    }

    if content.is_empty() {
        content.push(AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(String::new()),
            ..empty_block()
        });
    }

    if let Some(usage) = resp.usage.as_ref()
        && usage
            .output_tokens_details
            .as_ref()
            .and_then(|details| details.reasoning_tokens)
            .is_some()
    {
        report.degraded(
            Side::Source,
            "/usage/output_tokens_details/reasoning_tokens",
            "Anthropic usage has no separate reasoning_tokens field",
        );
    }

    let usage = resp.usage.as_ref().map(|usage| {
        let cached = usage
            .input_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens)
            .unwrap_or(0);
        let cache_creation = usage
            .cache_creation_input_tokens
            .or_else(|| {
                usage.input_tokens_details.as_ref().and_then(|details| {
                    details.cache_creation_tokens.or(details.cache_write_tokens)
                })
            })
            .unwrap_or(0);
        let input = saturating_deduct(usage.input_tokens, &[Some(cached), Some(cache_creation)]);
        AnthropicUsage {
            input_tokens: Some(input),
            output_tokens: usage.output_tokens,
            cache_creation_input_tokens: positive(cache_creation),
            cache_read_input_tokens: positive(cached),
            service_tier: resp.service_tier.clone(),
            cache_creation: None,
            server_tool_use: None,
            extra: Map::new(),
        }
    });

    let stop_reason = match resp.status.as_str() {
        "incomplete" => match resp
            .incomplete_details
            .as_ref()
            .map(|details| details.reason.as_str())
        {
            Some("content_filter") => "refusal",
            _ => "max_tokens",
        },
        "failed" => {
            report.warn(
                WarningCode::Degraded,
                Side::Target,
                "/stop_reason",
                "Responses failed response is emitted as end_turn",
            );
            "end_turn"
        }
        _ => {
            if has_tool_use {
                "tool_use"
            } else {
                "end_turn"
            }
        }
    };

    let id = if resp.id.is_empty() {
        "msg_transcoded".to_string()
    } else {
        resp.id
    };

    let out = AnthropicResponse {
        id,
        type_: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: resp.model,
        stop_reason: Some(stop_reason.to_string()),
        stop_sequence: None,
        usage,
        container: None,
        extra: Map::new(),
    };

    Ok(Transcoded {
        body: serde_json::to_vec(&out)?,
        report,
    })
}

fn sanitize_read_input(name: Option<&str>, mut input: Value) -> Value {
    if name != Some("Read") {
        return input;
    }
    if let Some(object) = input.as_object_mut()
        && object.get("pages") == Some(&Value::String(String::new()))
    {
        object.remove("pages");
    }
    input
}

fn output_item_text(item: &crate::wire::openai_responses::ResponsesOutput) -> Option<String> {
    let text = item
        .content
        .iter()
        .filter_map(|part| part.text.as_deref())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn responses_server_tool_id(
    item: &crate::wire::openai_responses::ResponsesOutput,
    index: usize,
) -> String {
    let raw = item
        .id
        .as_deref()
        .or(item.call_id.as_deref())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("web_search_{index}"));
    if raw.starts_with("srvtoolu_") {
        raw
    } else {
        format!("srvtoolu_{raw}")
    }
}

fn responses_server_tool_input(item: &crate::wire::openai_responses::ResponsesOutput) -> Value {
    let Some(action) = item.action.as_ref() else {
        return json!({"query": ""});
    };
    if let Some(query) = action.get("query").and_then(Value::as_str) {
        return json!({"query": query});
    }
    action.clone()
}

fn responses_server_tool_results(item: &crate::wire::openai_responses::ResponsesOutput) -> Value {
    let value = item
        .extra
        .get("results")
        .or_else(|| item.extra.get("sources"))
        .or_else(|| item.extra.get("content"))
        .or_else(|| {
            item.action
                .as_ref()
                .and_then(|action| action.get("results"))
        })
        .cloned()
        .unwrap_or_else(|| json!([]));
    normalize_results(value)
}

fn normalize_results(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items),
        Value::Null => json!([]),
        Value::Object(mut object) => {
            if let Some(Value::Array(items)) = object
                .remove("results")
                .or_else(|| object.remove("sources"))
            {
                Value::Array(items)
            } else if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|type_| type_.ends_with("_error"))
            {
                Value::Object(object)
            } else {
                json!([Value::Object(object)])
            }
        }
        other => json!([other]),
    }
}

fn empty_block() -> AnthropicContentBlock {
    AnthropicContentBlock {
        type_: String::new(),
        cache_control: None,
        text: None,
        thinking: None,
        signature: None,
        data: None,
        source: None,
        id: None,
        name: None,
        input: None,
        tool_use_id: None,
        content: None,
        is_error: None,
        citations: Vec::new(),
        extra: Map::new(),
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

fn map_error_type(error_type: Option<&str>) -> String {
    match error_type {
        Some("invalid_request_error") => "invalid_request_error",
        Some("authentication_error") => "authentication_error",
        Some("rate_limit_error") => "rate_limit_error",
        _ => "api_error",
    }
    .to_string()
}
