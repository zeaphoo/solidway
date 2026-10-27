//! Anthropic response -> Chat response。

use serde_json::{Map, Value, json};

use crate::common::empty;
use crate::registry::{Plan, Transcoded};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{AnthropicError, AnthropicResponse};
use crate::wire::openai_chat::{
    ChatChoice, ChatContent, ChatContentPart, ChatError, ChatErrorBody, ChatFunctionCall,
    ChatMessage, ChatResponse, ChatTokenDetails, ChatToolCall, ChatUsage,
};

pub(super) fn convert(body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
    if empty::is_empty_body(body) {
        return Ok(Transcoded {
            body: serde_json::to_vec(&empty::chat_response())?,
            report: Report::default(),
        });
    }

    let value: Value = serde_json::from_slice(body)?;

    if value.get("type").and_then(Value::as_str) == Some("error") {
        let err: AnthropicError = serde_json::from_value(value)?;
        let out = ChatError {
            error: ChatErrorBody {
                message: err.error.message,
                type_: Some(map_error_type(&err.error.type_)),
                param: None,
                code: Some(err.error.type_),
                extra: err.error.extra,
            },
        };
        return Ok(Transcoded {
            body: serde_json::to_vec(&out)?,
            report: Report::default(),
        });
    }

    let resp: AnthropicResponse = serde_json::from_value(value)?;
    let mut report = Report::default();

    if resp.stop_sequence.is_some() {
        report.degraded(
            Side::Source,
            "/stop_sequence",
            "Chat has no stop_sequence field",
        );
    }
    if resp.container.is_some() {
        report.degraded(Side::Source, "/container", "Chat has no container field");
    }
    for key in resp.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Anthropic response field is not mapped to Chat",
        );
    }

    let mut text_parts = Vec::new();
    let mut reasoning_parts = Vec::new();
    let mut citations = Vec::new();
    let mut tool_calls = Vec::new();

    for (index, block) in resp.content.iter().enumerate() {
        if block.cache_control.is_some() {
            report.degraded(
                Side::Source,
                format!("/content/{index}/cache_control"),
                "Chat has no cache_control field",
            );
        }
        if !block.citations.is_empty() {
            report.degraded(
                Side::Source,
                format!("/content/{index}/citations"),
                "Chat has no Anthropic citations field",
            );
        }
        match block.type_.as_str() {
            "text" => {
                if let Some(text) = block.text.as_deref().filter(|text| !text.is_empty()) {
                    text_parts.push(text.to_string());
                }
                citations.extend(block.citations.clone());
            }
            "thinking" => {
                if let Some(thinking) = block.thinking.as_deref().filter(|text| !text.is_empty()) {
                    reasoning_parts.push(thinking.to_string());
                }
                report.degraded(
                    Side::Target,
                    format!("/content/{index}/signature"),
                    "Anthropic thinking signature has no Chat field and is dropped",
                );
            }
            "redacted_thinking" => {
                report.degraded(
                    Side::Target,
                    format!("/content/{index}"),
                    "Anthropic redacted_thinking has no Chat equivalent",
                );
            }
            "tool_use" => {
                let input = block.input.clone().unwrap_or_else(|| json!({}));
                let arguments = serde_json::to_string(&input)?;
                tool_calls.push(ChatToolCall {
                    index: None,
                    id: block.id.clone(),
                    type_: Some("function".to_string()),
                    function: ChatFunctionCall {
                        name: block.name.clone(),
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
                    format!("/content/{index}/type"),
                    format!("Anthropic content block {other} has no Chat equivalent"),
                )?;
            }
        }
    }

    if text_parts.is_empty() && tool_calls.is_empty() && !reasoning_parts.is_empty() {
        text_parts.push(reasoning_parts.join("\n"));
        report.degraded(
            Side::Target,
            "/content",
            "reasoning-only response is surfaced as Anthropic text",
        );
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
                    image_url: None,
                    input_audio: None,
                    file: None,
                    refusal: None,
                    prompt_cache_breakpoint: None,
                    extra: Map::new(),
                })
                .collect(),
        ))
    };

    let reasoning_content = if reasoning_parts.is_empty() {
        None
    } else {
        Some(reasoning_parts.join("\n"))
    };

    let finish_reason = map_finish_reason(resp.stop_reason.as_deref(), &mut report);

    if let Some(usage) = resp.usage.as_ref() {
        if usage.cache_creation.is_some() {
            report.degraded(
                Side::Source,
                "/usage/cache_creation",
                "Chat usage has no cache_creation breakdown",
            );
        }
        if usage.server_tool_use.is_some() {
            report.degraded(
                Side::Source,
                "/usage/server_tool_use",
                "Chat usage has no server_tool_use breakdown",
            );
        }
    }

    let usage = resp.usage.as_ref().map(|usage| {
        let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
        let cache_creation = usage.cache_creation_input_tokens.unwrap_or(0);
        let prompt = usage.input_tokens.unwrap_or(0) + cache_read + cache_creation;
        let completion = usage.output_tokens.unwrap_or(0);
        ChatUsage {
            prompt_tokens: Some(prompt),
            completion_tokens: Some(completion),
            total_tokens: Some(prompt + completion),
            prompt_tokens_details: if cache_read > 0 {
                Some(ChatTokenDetails {
                    cached_tokens: Some(cache_read),
                    audio_tokens: None,
                    cache_creation_tokens: None,
                    cache_write_tokens: None,
                    reasoning_tokens: None,
                    accepted_prediction_tokens: None,
                    rejected_prediction_tokens: None,
                    extra: Map::new(),
                })
            } else {
                None
            },
            completion_tokens_details: None,
            extra: Map::new(),
        }
    });

    report.warn(
        WarningCode::Synthesized,
        Side::Target,
        "/created",
        "Anthropic response has no created timestamp; using 0",
    );

    let id = if resp.id.is_empty() {
        "chatcmpl_transcoded".to_string()
    } else {
        resp.id
    };

    let out = ChatResponse {
        id,
        object: "chat.completion".to_string(),
        created: 0,
        model: resp.model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content,
                refusal: None,
                reasoning_content,
                reasoning: None,
                name: None,
                tool_calls,
                tool_call_id: None,
                function_call: None,
                extra: if citations.is_empty() {
                    Map::new()
                } else {
                    let mut extra = Map::new();
                    extra.insert("citations".to_string(), Value::Array(citations));
                    extra
                },
            },
            finish_reason,
            logprobs: None,
            extra: Map::new(),
        }],
        usage,
        system_fingerprint: None,
        service_tier: resp
            .usage
            .as_ref()
            .and_then(|usage| usage.service_tier.clone()),
        extra: Map::new(),
    };

    Ok(Transcoded {
        body: serde_json::to_vec(&out)?,
        report,
    })
}

fn map_finish_reason(stop_reason: Option<&str>, report: &mut Report) -> String {
    match stop_reason {
        Some("end_turn") | Some("stop_sequence") | Some("pause_turn") => "stop".to_string(),
        Some("max_tokens") => "length".to_string(),
        Some("tool_use") => "tool_calls".to_string(),
        Some("refusal") => "content_filter".to_string(),
        Some(other) => {
            report.degraded(
                Side::Target,
                "/choices/0/finish_reason",
                format!("unknown Anthropic stop_reason {other}; using stop"),
            );
            "stop".to_string()
        }
        None => {
            report.warn(
                WarningCode::Synthesized,
                Side::Target,
                "/choices/0/finish_reason",
                "Anthropic response has no stop_reason; using stop",
            );
            "stop".to_string()
        }
    }
}

fn map_error_type(error_type: &str) -> String {
    match error_type {
        "invalid_request_error" => "invalid_request_error",
        "authentication_error" => "authentication_error",
        "rate_limit_error" => "rate_limit_error",
        _ => "server_error",
    }
    .to_string()
}
