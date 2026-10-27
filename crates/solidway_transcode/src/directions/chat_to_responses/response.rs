//! Responses response -> Chat response。

use serde_json::{Map, Value};

use crate::common::empty;
use crate::registry::{Plan, Transcoded};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::openai_chat::{
    ChatChoice, ChatContent, ChatContentPart, ChatError, ChatErrorBody, ChatFunctionCall,
    ChatMessage, ChatResponse, ChatTokenDetails, ChatToolCall, ChatUsage,
};
use crate::wire::openai_responses::{ResponsesErrorEnvelope, ResponsesResponse};

pub(super) fn convert(body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
    if empty::is_empty_body(body) {
        return Ok(Transcoded {
            body: serde_json::to_vec(&empty::chat_response())?,
            report: Report::default(),
        });
    }

    let value: Value = serde_json::from_slice(body)?;

    if value.get("error").is_some() {
        let err: ResponsesErrorEnvelope = serde_json::from_value(value)?;
        return Ok(Transcoded {
            body: serde_json::to_vec(&chat_error(
                err.error.message,
                err.error.type_,
                err.error.code,
                err.error.param,
                err.error.extra,
            ))?,
            report: Report::default(),
        });
    }

    let resp: ResponsesResponse = serde_json::from_value(value)?;
    if resp.status == "failed"
        && let Some(error) = resp.error.as_ref()
    {
        return Ok(Transcoded {
            body: serde_json::to_vec(&chat_error(
                error.message.clone(),
                error.type_.clone(),
                error.code.clone(),
                error.param.clone(),
                error.extra.clone(),
            ))?,
            report: Report::default(),
        });
    }

    let mut report = Report::default();
    for key in resp.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Responses field is not mapped to Chat",
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
                "Responses request echo field has no Chat response equivalent",
            );
        }
    }

    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut refusal_parts = Vec::new();
    let mut annotations = Vec::new();
    let mut tool_calls = Vec::new();

    for (index, item) in resp.output.iter().enumerate() {
        for key in item.extra.keys() {
            report.warn(
                WarningCode::UnknownField,
                Side::Source,
                format!("/output/{index}/{key}"),
                "unknown Responses output field is not mapped to Chat",
            );
        }
        if item.namespace.is_some() {
            report.degraded(
                Side::Source,
                format!("/output/{index}/namespace"),
                "Chat has no namespace field",
            );
        }
        if item.input.is_some() || item.action.is_some() {
            report.degraded(
                Side::Source,
                format!("/output/{index}"),
                "Responses item input/action has no Chat equivalent",
            );
        }
        match item.type_.as_str() {
            "message" => {
                for (part_index, part) in item.content.iter().enumerate() {
                    if !part.annotations.is_empty() {
                        annotations.extend(part.annotations.clone());
                        report.degraded(
                            Side::Source,
                            format!("/output/{index}/content/{part_index}/annotations"),
                            "Chat annotations are moved to message.extra.annotations",
                        );
                    }
                    if part.logprobs.is_some() {
                        report.degraded(
                            Side::Source,
                            format!("/output/{index}/content/{part_index}/logprobs"),
                            "Chat has no logprobs field in output content",
                        );
                    }
                    if part.prompt_cache_breakpoint.is_some() {
                        report.degraded(
                            Side::Source,
                            format!("/output/{index}/content/{part_index}/prompt_cache_breakpoint"),
                            "Chat has no prompt_cache_breakpoint field",
                        );
                    }
                    match part.type_.as_str() {
                        "output_text" => {
                            if let Some(text) = part.text.as_deref().filter(|text| !text.is_empty())
                            {
                                text_parts.push(text.to_string());
                            }
                        }
                        "refusal" => {
                            if let Some(refusal) =
                                part.refusal.as_deref().filter(|text| !text.is_empty())
                            {
                                refusal_parts.push(refusal.to_string());
                            }
                        }
                        other => report.unsupported(
                            plan.policy,
                            Side::Target,
                            format!("/output/{index}/content/{other}"),
                            format!("Responses content part {other} has no Chat equivalent"),
                        )?,
                    }
                }
            }
            "reasoning" => {
                for summary in &item.summary {
                    if !summary.text.is_empty() {
                        reasoning_parts.push(summary.text.clone());
                    }
                }
                if item.encrypted_content.is_some() {
                    report.degraded(
                        Side::Target,
                        format!("/output/{index}/encrypted_content"),
                        "Responses encrypted_content has no Chat field",
                    );
                }
            }
            "custom_tool_call" => {
                let input = match item.input.as_ref() {
                    Some(Value::String(text)) => serde_json::json!({"input": text}),
                    Some(value) => serde_json::json!({"input": value}),
                    None => serde_json::json!({"input": ""}),
                };
                tool_calls.push(ChatToolCall {
                    index: None,
                    id: item.call_id.clone(),
                    type_: Some("function".to_string()),
                    function: ChatFunctionCall {
                        name: item.name.clone(),
                        arguments: serde_json::to_string(&input)?,
                        extra: Map::new(),
                    },
                    extra: Map::new(),
                });
            }
            "tool_search_call" => {
                let arguments = match item.arguments.as_ref() {
                    Some(Value::String(text)) => text.clone(),
                    Some(value) => serde_json::to_string(value)?,
                    None => "{}".to_string(),
                };
                tool_calls.push(ChatToolCall {
                    index: None,
                    id: item.call_id.clone(),
                    type_: Some("function".to_string()),
                    function: ChatFunctionCall {
                        name: Some("tool_search".to_string()),
                        arguments,
                        extra: Map::new(),
                    },
                    extra: Map::new(),
                });
            }
            "web_search_call" => {
                // sub2api consumes server-side web search silently; its results
                // are already represented in the text output.
                report.degraded(
                    Side::Source,
                    format!("/output/{index}"),
                    "Responses web_search_call is consumed server-side",
                );
            }
            "x_search_call" => {
                let name = "x_search".to_string();
                let mut arguments = item.action.clone().unwrap_or_else(|| serde_json::json!({}));
                if let Some(object) = arguments.as_object_mut() {
                    if let Some(results) = item.extra.get("results") {
                        object.insert("results".to_string(), results.clone());
                    }
                    if let Some(citations) = item.extra.get("citations") {
                        object.insert("citations".to_string(), citations.clone());
                    }
                }
                tool_calls.push(ChatToolCall {
                    index: None,
                    id: item.call_id.clone(),
                    type_: Some("function".to_string()),
                    function: ChatFunctionCall {
                        name: Some(name),
                        arguments: serde_json::to_string(&arguments)?,
                        extra: Map::new(),
                    },
                    extra: Map::new(),
                });
            }
            "function_call" => {
                let arguments = match item.arguments.as_ref() {
                    Some(Value::String(text)) => text.clone(),
                    Some(value) => serde_json::to_string(value)?,
                    None => String::new(),
                };
                tool_calls.push(ChatToolCall {
                    index: None,
                    id: item.call_id.clone(),
                    type_: Some("function".to_string()),
                    function: ChatFunctionCall {
                        name: item.name.clone(),
                        arguments,
                        extra: Map::new(),
                    },
                    extra: Map::new(),
                });
            }
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Target,
                    format!("/output/{index}/type"),
                    format!("Responses output item {other} has no Chat equivalent"),
                )?;
            }
        }
    }

    let content = if text_parts.is_empty() {
        if tool_calls.is_empty() {
            Some(ChatContent::Text(String::new()))
        } else {
            None
        }
    } else if text_parts.len() == 1 && tool_calls.is_empty() {
        Some(ChatContent::Text(text_parts[0].clone()))
    } else {
        Some(ChatContent::Parts(
            text_parts
                .iter()
                .map(|text| ChatContentPart {
                    type_: "text".to_string(),
                    text: Some(text.clone()),
                    ..empty_part()
                })
                .collect(),
        ))
    };

    let finish_reason = finish_reason(&resp, !tool_calls.is_empty(), &mut report);
    let usage = resp.usage.as_ref().map(|usage| ChatUsage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage
            .total_tokens
            .or_else(|| Some(usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0))),
        prompt_tokens_details: usage.input_tokens_details.as_ref().and_then(|details| {
            let mapped = ChatTokenDetails {
                cached_tokens: details.cached_tokens.filter(|value| *value > 0),
                audio_tokens: details.audio_tokens.filter(|value| *value > 0),
                cache_creation_tokens: details.cache_creation_tokens.filter(|value| *value > 0),
                cache_write_tokens: details.cache_write_tokens.filter(|value| *value > 0),
                ..empty_details()
            };
            (!chat_details_empty(&mapped)).then_some(mapped)
        }),
        completion_tokens_details: usage.output_tokens_details.as_ref().and_then(|details| {
            let mapped = ChatTokenDetails {
                reasoning_tokens: details.reasoning_tokens.filter(|value| *value > 0),
                audio_tokens: details.audio_tokens.filter(|value| *value > 0),
                accepted_prediction_tokens: details
                    .accepted_prediction_tokens
                    .filter(|value| *value > 0),
                rejected_prediction_tokens: details
                    .rejected_prediction_tokens
                    .filter(|value| *value > 0),
                ..empty_details()
            };
            (!chat_details_empty(&mapped)).then_some(mapped)
        }),
        extra: Map::new(),
    });

    let id = if resp.id.is_empty() {
        "chatcmpl_transcoded".to_string()
    } else {
        resp.id
    };

    let out = ChatResponse {
        id,
        object: "chat.completion".to_string(),
        created: resp.created_at,
        model: resp.model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content,
                refusal: if refusal_parts.is_empty() {
                    None
                } else {
                    Some(refusal_parts.join("\n"))
                },
                reasoning_content: if reasoning_parts.is_empty() {
                    None
                } else {
                    Some(reasoning_parts.join("\n"))
                },
                reasoning: None,
                name: None,
                tool_calls,
                tool_call_id: None,
                function_call: None,
                extra: if annotations.is_empty() {
                    Map::new()
                } else {
                    let mut extra = Map::new();
                    extra.insert("annotations".to_string(), Value::Array(annotations));
                    extra
                },
            },
            finish_reason,
            logprobs: None,
            extra: Map::new(),
        }],
        usage,
        system_fingerprint: None,
        service_tier: resp.service_tier,
        extra: Map::new(),
    };

    Ok(Transcoded {
        body: serde_json::to_vec(&out)?,
        report,
    })
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

fn chat_details_empty(details: &ChatTokenDetails) -> bool {
    details.cached_tokens.is_none()
        && details.audio_tokens.is_none()
        && details.cache_creation_tokens.is_none()
        && details.cache_write_tokens.is_none()
        && details.reasoning_tokens.is_none()
        && details.accepted_prediction_tokens.is_none()
        && details.rejected_prediction_tokens.is_none()
        && details.extra.is_empty()
}

fn empty_details() -> ChatTokenDetails {
    ChatTokenDetails {
        cached_tokens: None,
        audio_tokens: None,
        cache_creation_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
        accepted_prediction_tokens: None,
        rejected_prediction_tokens: None,
        extra: Map::new(),
    }
}

fn chat_error(
    message: String,
    type_: Option<String>,
    code: Option<String>,
    param: Option<String>,
    extra: Map<String, Value>,
) -> ChatError {
    ChatError {
        error: ChatErrorBody {
            message,
            type_: Some(type_.unwrap_or_else(|| "server_error".to_string())),
            param,
            code,
            extra,
        },
    }
}

fn finish_reason(resp: &ResponsesResponse, has_tool_calls: bool, report: &mut Report) -> String {
    match resp.status.as_str() {
        "completed" => {
            if has_tool_calls {
                "tool_calls".to_string()
            } else {
                "stop".to_string()
            }
        }
        "incomplete" => match resp
            .incomplete_details
            .as_ref()
            .map(|details| details.reason.as_str())
        {
            Some("content_filter") => "content_filter".to_string(),
            Some("max_output_tokens") => "length".to_string(),
            other => {
                report.warn(
                    WarningCode::Degraded,
                    Side::Target,
                    "/choices/0/finish_reason",
                    format!("unknown Responses incomplete reason {other:?}; using length"),
                );
                "length".to_string()
            }
        },
        "failed" => {
            report.warn(
                WarningCode::Degraded,
                Side::Target,
                "/choices/0/finish_reason",
                "Responses failed without error body; using stop",
            );
            "stop".to_string()
        }
        other => {
            report.warn(
                WarningCode::Degraded,
                Side::Target,
                "/choices/0/finish_reason",
                format!("unknown Responses status {other}; using stop"),
            );
            "stop".to_string()
        }
    }
}
