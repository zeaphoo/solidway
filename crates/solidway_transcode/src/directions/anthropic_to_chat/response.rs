//! Chat response -> Anthropic response。

use serde_json::{Map, Value, json};

use crate::common::empty;
use crate::common::usage::{positive, saturating_deduct};
use crate::registry::{Plan, Transcoded};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{
    AnthropicContentBlock, AnthropicError, AnthropicErrorBody, AnthropicResponse, AnthropicSource,
    AnthropicUsage,
};
use crate::wire::openai_chat::{ChatError, ChatResponse};

pub(super) fn convert(body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
    if empty::is_empty_body(body) {
        return Ok(Transcoded {
            body: serde_json::to_vec(&empty::anthropic_response())?,
            report: Report::default(),
        });
    }

    let value: Value = serde_json::from_slice(body)?;

    if value.get("error").is_some() {
        let err: ChatError = serde_json::from_value(value)?;
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
        let out = AnthropicError {
            type_: "error".to_string(),
            error: AnthropicErrorBody {
                type_: map_error_type(err.error.type_.as_deref()),
                message: err.error.message,
                extra,
            },
            request_id: None,
            extra: Map::new(),
        };
        return Ok(Transcoded {
            body: serde_json::to_vec(&out)?,
            report: Report::default(),
        });
    }

    let resp: ChatResponse = serde_json::from_value(value)?;
    let mut report = Report::default();
    if resp.choices.len() > 1 {
        report.degraded(
            Side::Source,
            "/choices",
            "Anthropic responses have one choice; extra Chat choices are dropped",
        );
    }
    if resp.system_fingerprint.is_some() {
        report.degraded(
            Side::Source,
            "/system_fingerprint",
            "Anthropic has no system_fingerprint field",
        );
    }
    for key in resp.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Chat response field is not mapped to Anthropic",
        );
    }
    let Some(choice) = resp.choices.first() else {
        let id = if resp.id.is_empty() {
            "msg_transcoded".to_string()
        } else {
            resp.id
        };
        let out = AnthropicResponse {
            id,
            type_: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![AnthropicContentBlock {
                type_: "text".to_string(),
                text: Some(String::new()),
                ..empty_block()
            }],
            model: resp.model,
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: None,
            container: None,
            extra: Map::new(),
        };
        return Ok(Transcoded {
            body: serde_json::to_vec(&out)?,
            report,
        });
    };
    for key in choice.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/choices/0/{key}"),
            "unknown Chat choice field is not mapped to Anthropic",
        );
    }
    if choice.logprobs.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/logprobs",
            "Anthropic has no logprobs field",
        );
    }
    if choice.message.name.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/message/name",
            "Anthropic has no message name field",
        );
    }
    if choice.message.tool_call_id.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/message/tool_call_id",
            "Anthropic has no assistant tool_call_id field",
        );
    }
    if choice.message.function_call.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/message/function_call",
            "legacy Chat function_call has no Anthropic equivalent",
        );
    }
    for key in choice.message.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/choices/0/message/{key}"),
            "unknown Chat message field is not mapped to Anthropic",
        );
    }

    let mut content = Vec::new();
    if let Some(reasoning) = choice
        .message
        .reasoning_content
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        content.push(AnthropicContentBlock {
            type_: "thinking".to_string(),
            thinking: Some(reasoning.to_string()),
            signature: Some(String::new()),
            ..empty_block()
        });
        report.degraded(
            Side::Target,
            "/content/0/signature",
            "Chat reasoning has no Anthropic signature; emitting an empty signature",
        );
    }

    if let Some(refusal) = choice
        .message
        .refusal
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        content.push(AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(refusal.to_string()),
            ..empty_block()
        });
        report.degraded(
            Side::Target,
            "/choices/0/message/refusal",
            "Chat refusal is emitted as an Anthropic text block",
        );
    }

    match choice.message.content.as_ref() {
        None => {}
        Some(crate::wire::openai_chat::ChatContent::Text(text)) => {
            if !text.is_empty() {
                content.push(AnthropicContentBlock {
                    type_: "text".to_string(),
                    text: Some(text.clone()),
                    ..empty_block()
                });
            }
        }
        Some(crate::wire::openai_chat::ChatContent::Parts(parts)) => {
            for (index, part) in parts.iter().enumerate() {
                match part.type_.as_str() {
                    "text" => {
                        if let Some(text) = part.text.as_deref().filter(|text| !text.is_empty()) {
                            content.push(AnthropicContentBlock {
                                type_: "text".to_string(),
                                text: Some(text.to_string()),
                                citations: part
                                    .extra
                                    .get("annotations")
                                    .and_then(Value::as_array)
                                    .cloned()
                                    .unwrap_or_default(),
                                ..empty_block()
                            });
                        }
                    }
                    "image_url" => {
                        if let Some(image) = part.image_url.as_ref() {
                            let source = image_source(&image.url);
                            content.push(AnthropicContentBlock {
                                type_: "image".to_string(),
                                source: Some(source),
                                ..empty_block()
                            });
                        }
                    }
                    "file" => {
                        if let Some(file) = part.file.as_ref() {
                            if let Some(source) = file_source(file) {
                                content.push(AnthropicContentBlock {
                                    type_: "document".to_string(),
                                    source: Some(source),
                                    ..empty_block()
                                });
                            } else {
                                report.degraded(
                                    Side::Target,
                                    format!("/content/{index}"),
                                    "Chat file has neither file_data nor file_id",
                                );
                            }
                        }
                    }
                    other => {
                        report.unsupported(
                            plan.policy,
                            Side::Target,
                            format!("/content/{index}/type"),
                            format!("Chat content part {other} has no Anthropic equivalent"),
                        )?;
                    }
                }
            }
        }
    }

    for (index, call) in choice.message.tool_calls.iter().enumerate() {
        let input =
            serde_json::from_str::<Value>(&call.function.arguments).unwrap_or_else(|_| json!({}));
        if !input.is_object() {
            report.degraded(
                Side::Target,
                format!("/content/tool_use/{index}/input"),
                "Chat tool arguments are not a JSON object; using an empty object",
            );
        }
        let id = call
            .id
            .clone()
            .filter(|id| !id.is_empty())
            .unwrap_or_else(|| format!("toolu_{index}"));
        if call.id.as_deref().is_none_or(str::is_empty) {
            report.degraded(
                Side::Target,
                format!("/content/tool_use/{index}/id"),
                "Chat tool call has no id; generated one for Anthropic tool_use",
            );
        }
        content.push(AnthropicContentBlock {
            type_: "tool_use".to_string(),
            id: Some(id),
            name: call.function.name.clone(),
            input: Some(if input.is_object() { input } else { json!({}) }),
            ..empty_block()
        });
    }

    let has_visible = content.iter().any(|block| {
        matches!(
            block.type_.as_str(),
            "text" | "tool_use" | "image" | "document"
        )
    });
    if !has_visible
        && let Some(reasoning) = choice
            .message
            .reasoning_content
            .as_deref()
            .filter(|text| !text.is_empty())
    {
        content.push(AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(reasoning.to_string()),
            ..empty_block()
        });
    }
    if content.is_empty() {
        content.push(AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(String::new()),
            ..empty_block()
        });
    }

    if let Some(usage) = resp.usage.as_ref() {
        for (pointer, present) in [
            (
                "/usage/prompt_tokens_details/audio_tokens",
                usage
                    .prompt_tokens_details
                    .as_ref()
                    .and_then(|details| details.audio_tokens)
                    .is_some(),
            ),
            (
                "/usage/completion_tokens_details/audio_tokens",
                usage
                    .completion_tokens_details
                    .as_ref()
                    .and_then(|details| details.audio_tokens)
                    .is_some(),
            ),
            (
                "/usage/completion_tokens_details/accepted_prediction_tokens",
                usage
                    .completion_tokens_details
                    .as_ref()
                    .and_then(|details| details.accepted_prediction_tokens)
                    .is_some(),
            ),
            (
                "/usage/completion_tokens_details/rejected_prediction_tokens",
                usage
                    .completion_tokens_details
                    .as_ref()
                    .and_then(|details| details.rejected_prediction_tokens)
                    .is_some(),
            ),
        ] {
            if present {
                report.degraded(
                    Side::Source,
                    pointer,
                    "Anthropic usage has no equivalent breakdown",
                );
            }
        }
    }

    let usage = resp.usage.as_ref().map(|usage| {
        let cached = usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens)
            .unwrap_or(0);
        let cache_creation = usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cache_write_tokens.or(details.cache_creation_tokens))
            .unwrap_or(0);
        let input = saturating_deduct(usage.prompt_tokens, &[Some(cached), Some(cache_creation)]);
        AnthropicUsage {
            input_tokens: Some(input),
            output_tokens: usage.completion_tokens,
            cache_creation_input_tokens: positive(cache_creation),
            cache_read_input_tokens: positive(cached),
            service_tier: resp.service_tier.clone(),
            cache_creation: None,
            server_tool_use: None,
            extra: Map::new(),
        }
    });

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
        stop_reason: Some(map_finish_reason(&choice.finish_reason, &mut report)),
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

fn image_source(url: &str) -> AnthropicSource {
    if let Some(rest) = url.strip_prefix("data:")
        && let Some((meta, data)) = rest.split_once(',')
    {
        return AnthropicSource {
            type_: "base64".to_string(),
            media_type: Some(meta.strip_suffix(";base64").unwrap_or(meta).to_string()),
            data: Some(data.to_string()),
            url: None,
            file_id: None,
            extra: Map::new(),
        };
    }
    AnthropicSource {
        type_: "url".to_string(),
        media_type: None,
        data: None,
        url: Some(url.to_string()),
        file_id: None,
        extra: Map::new(),
    }
}

fn file_source(file: &crate::wire::openai_chat::ChatFile) -> Option<AnthropicSource> {
    if let Some(data) = file.file_data.as_deref()
        && let Some(rest) = data.strip_prefix("data:")
        && let Some((meta, payload)) = rest.split_once(',')
    {
        return Some(AnthropicSource {
            type_: "base64".to_string(),
            media_type: Some(meta.strip_suffix(";base64").unwrap_or(meta).to_string()),
            data: Some(payload.to_string()),
            url: None,
            file_id: None,
            extra: Map::new(),
        });
    }
    file.file_id.as_ref().map(|file_id| AnthropicSource {
        type_: "file".to_string(),
        media_type: None,
        data: None,
        url: None,
        file_id: Some(file_id.clone()),
        extra: Map::new(),
    })
}

fn map_finish_reason(finish_reason: &str, report: &mut Report) -> String {
    match finish_reason {
        "stop" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        "content_filter" => "refusal",
        other => {
            report.warn(
                WarningCode::Degraded,
                Side::Target,
                "/stop_reason",
                format!("unknown Chat finish_reason {other}; using end_turn"),
            );
            "end_turn"
        }
    }
    .to_string()
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
