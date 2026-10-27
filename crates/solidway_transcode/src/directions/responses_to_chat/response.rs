//! Chat response -> Responses response。

use serde_json::{Map, Value};

use crate::common::empty;
use crate::registry::{Plan, ResponseContext, Transcoded};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::openai_chat::{ChatContent, ChatError, ChatResponse};
use crate::wire::openai_responses::{
    ResponsesContentPart, ResponsesError, ResponsesErrorEnvelope, ResponsesIncompleteDetails,
    ResponsesInputTokensDetails, ResponsesOutput, ResponsesOutputTokensDetails, ResponsesResponse,
    ResponsesSummary, ResponsesUsage,
};

pub(super) fn convert(
    body: &[u8],
    plan: &Plan,
    context: &ResponseContext,
) -> Result<Transcoded<Vec<u8>>, Error> {
    if empty::is_empty_body(body) {
        return Ok(Transcoded {
            body: serde_json::to_vec(&empty::responses_response())?,
            report: Report::default(),
        });
    }

    let value: Value = serde_json::from_slice(body)?;

    if value.get("error").is_some() {
        let err: ChatError = serde_json::from_value(value)?;
        let out = ResponsesErrorEnvelope {
            error: ResponsesError {
                code: err.error.code,
                message: err.error.message,
                param: err.error.param,
                type_: err.error.type_,
                extra: err.error.extra,
            },
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
            "Responses responses have one output; extra Chat choices are dropped",
        );
    }
    if resp.system_fingerprint.is_some() {
        report.degraded(
            Side::Source,
            "/system_fingerprint",
            "Responses has no system_fingerprint field",
        );
    }
    for key in resp.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Chat response field is not mapped to Responses",
        );
    }
    let Some(choice) = resp.choices.first() else {
        let id = if resp.id.is_empty() {
            "resp_transcoded".to_string()
        } else {
            resp.id
        };
        let out = ResponsesResponse {
            id,
            object: "response".to_string(),
            created_at: 1,
            model: resp.model,
            status: "completed".to_string(),
            output: vec![ResponsesOutput {
                type_: "message".to_string(),
                id: Some("msg_transcoded".to_string()),
                role: Some("assistant".to_string()),
                status: Some("completed".to_string()),
                content: vec![ResponsesContentPart {
                    type_: "output_text".to_string(),
                    text: Some(String::new()),
                    ..empty_part()
                }],
                ..empty_output()
            }],
            usage: None,
            service_tier: None,
            incomplete_details: None,
            error: None,
            instructions: None,
            metadata: None,
            parallel_tool_calls: None,
            previous_response_id: None,
            reasoning: None,
            store: None,
            temperature: None,
            text: None,
            tool_choice: None,
            tools: Vec::new(),
            top_p: None,
            truncation: None,
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
            "unknown Chat choice field is not mapped to Responses",
        );
    }
    if choice.logprobs.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/logprobs",
            "Responses has no logprobs field",
        );
    }
    if choice.message.name.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/message/name",
            "Responses has no message name field",
        );
    }
    if choice.message.function_call.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/message/function_call",
            "legacy Chat function_call has no Responses equivalent",
        );
    }
    if choice.message.tool_call_id.is_some() {
        report.degraded(
            Side::Source,
            "/choices/0/message/tool_call_id",
            "Responses has no assistant tool_call_id field",
        );
    }
    for key in choice.message.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/choices/0/message/{key}"),
            "unknown Chat message field is not mapped to Responses",
        );
    }

    let mut output = Vec::new();

    if let Some(reasoning) = choice
        .message
        .reasoning_content
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        output.push(ResponsesOutput {
            type_: "reasoning".to_string(),
            id: Some(format!("{}-reasoning", resp.id)),
            summary: vec![ResponsesSummary {
                type_: "summary_text".to_string(),
                text: reasoning.to_string(),
                extra: Map::new(),
            }],
            ..empty_output()
        });
        report.degraded(
            Side::Target,
            "/output/reasoning/encrypted_content",
            "Chat reasoning has no Responses encrypted_content",
        );
    }

    let mut message_parts = Vec::new();
    if let Some(refusal) = choice
        .message
        .refusal
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        message_parts.push(ResponsesContentPart {
            type_: "refusal".to_string(),
            refusal: Some(refusal.to_string()),
            ..empty_part()
        });
        report.degraded(
            Side::Target,
            "/choices/0/message/refusal",
            "Chat refusal is emitted as a Responses refusal part",
        );
    }
    match choice.message.content.as_ref() {
        None => {}
        Some(ChatContent::Text(text)) => {
            if !text.is_empty() {
                message_parts.push(ResponsesContentPart {
                    type_: "output_text".to_string(),
                    text: Some(text.clone()),
                    ..empty_part()
                });
            }
        }
        Some(ChatContent::Parts(parts)) => {
            for (index, part) in parts.iter().enumerate() {
                match part.type_.as_str() {
                    "text" => {
                        if let Some(text) = part.text.as_ref() {
                            message_parts.push(ResponsesContentPart {
                                type_: "output_text".to_string(),
                                text: Some(text.clone()),
                                ..empty_part()
                            });
                        }
                    }
                    "refusal" => {
                        if let Some(refusal) = part.refusal.as_ref() {
                            message_parts.push(ResponsesContentPart {
                                type_: "refusal".to_string(),
                                refusal: Some(refusal.clone()),
                                ..empty_part()
                            });
                        }
                    }
                    other => {
                        report.unsupported(
                            plan.policy,
                            Side::Target,
                            format!("/content/{index}/type"),
                            format!("Chat content part {other} has no Responses output equivalent"),
                        )?;
                    }
                }
            }
        }
    }
    if !message_parts.is_empty() {
        output.push(ResponsesOutput {
            type_: "message".to_string(),
            id: Some(format!("{}-message", resp.id)),
            role: Some("assistant".to_string()),
            status: Some("completed".to_string()),
            content: message_parts,
            ..empty_output()
        });
    }

    for (index, call) in choice.message.tool_calls.iter().enumerate() {
        let name = call.function.name.as_deref().unwrap_or("");
        if name == "tool_search" {
            let arguments = serde_json::from_str::<Value>(&call.function.arguments)
                .unwrap_or_else(|_| Value::String(call.function.arguments.clone()));
            output.push(ResponsesOutput {
                type_: "tool_search_call".to_string(),
                id: Some(format!("{}-call-{index}", resp.id)),
                call_id: call.id.clone(),
                name: None,
                arguments: Some(arguments),
                status: Some("completed".to_string()),
                ..empty_output()
            });
            continue;
        }
        if context.custom_tools.contains(name) {
            let input = extract_custom_input(&call.function.arguments);
            output.push(ResponsesOutput {
                type_: "custom_tool_call".to_string(),
                id: Some(format!("{}-call-{index}", resp.id)),
                call_id: call.id.clone(),
                name: Some(name.to_string()),
                input: Some(input),
                status: Some("completed".to_string()),
                ..empty_output()
            });
            continue;
        }
        let (namespace, item_name) = match context.namespaces.get(name) {
            Some(namespace) => (
                Some(namespace.namespace.clone()),
                Some(namespace.name.clone()),
            ),
            None => match call
                .function
                .name
                .as_deref()
                .and_then(|name| name.split_once("__"))
            {
                Some((namespace, child)) => (Some(namespace.to_string()), Some(child.to_string())),
                None => (None, call.function.name.clone()),
            },
        };
        output.push(ResponsesOutput {
            type_: "function_call".to_string(),
            id: Some(format!("{}-call-{index}", resp.id)),
            call_id: call.id.clone(),
            name: item_name,
            namespace,
            arguments: Some(Value::String(call.function.arguments.clone())),
            status: Some("completed".to_string()),
            ..empty_output()
        });
    }

    let (status, incomplete_details) = match choice.finish_reason.as_str() {
        "length" => (
            "incomplete".to_string(),
            Some(ResponsesIncompleteDetails {
                reason: "max_output_tokens".to_string(),
                extra: Map::new(),
            }),
        ),
        "content_filter" => (
            "incomplete".to_string(),
            Some(ResponsesIncompleteDetails {
                reason: "content_filter".to_string(),
                extra: Map::new(),
            }),
        ),
        "stop" | "tool_calls" => ("completed".to_string(), None),
        other => {
            report.warn(
                WarningCode::Degraded,
                Side::Target,
                "/status",
                format!("unknown Chat finish_reason {other}; using completed"),
            );
            ("completed".to_string(), None)
        }
    };

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
                    "Responses usage has no equivalent breakdown",
                );
            }
        }
        for key in usage.extra.keys() {
            report.warn(
                WarningCode::UnknownField,
                Side::Source,
                format!("/usage/{key}"),
                "unknown Chat usage field is not mapped to Responses",
            );
        }
    }

    let usage = resp.usage.as_ref().map(|usage| {
        let prompt = usage.prompt_tokens_details.as_ref();
        let completion = usage.completion_tokens_details.as_ref();
        ResponsesUsage {
            input_tokens: usage.prompt_tokens,
            output_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens.or_else(|| {
                Some(usage.prompt_tokens.unwrap_or(0) + usage.completion_tokens.unwrap_or(0))
            }),
            cache_creation_input_tokens: prompt
                .and_then(|details| details.cache_creation_tokens.or(details.cache_write_tokens)),
            input_tokens_details: (prompt.is_some()).then(|| ResponsesInputTokensDetails {
                cached_tokens: prompt
                    .and_then(|details| details.cached_tokens)
                    .filter(|value| *value > 0),
                audio_tokens: prompt
                    .and_then(|details| details.audio_tokens)
                    .filter(|value| *value > 0),
                cache_creation_tokens: prompt
                    .and_then(|details| details.cache_creation_tokens)
                    .filter(|value| *value > 0),
                cache_write_tokens: prompt
                    .and_then(|details| details.cache_write_tokens)
                    .filter(|value| *value > 0),
                extra: Map::new(),
            }),
            output_tokens_details: (completion.is_some()).then(|| ResponsesOutputTokensDetails {
                reasoning_tokens: completion
                    .and_then(|details| details.reasoning_tokens)
                    .filter(|value| *value > 0),
                audio_tokens: completion
                    .and_then(|details| details.audio_tokens)
                    .filter(|value| *value > 0),
                accepted_prediction_tokens: completion
                    .and_then(|details| details.accepted_prediction_tokens)
                    .filter(|value| *value > 0),
                rejected_prediction_tokens: completion
                    .and_then(|details| details.rejected_prediction_tokens)
                    .filter(|value| *value > 0),
                extra: Map::new(),
            }),
            extra: Map::new(),
        }
    });

    let id = if resp.id.is_empty() {
        "resp_transcoded".to_string()
    } else {
        resp.id
    };

    let out = ResponsesResponse {
        id,
        object: "response".to_string(),
        created_at: resp.created,
        model: resp.model,
        status,
        output,
        usage,
        service_tier: resp.service_tier,
        incomplete_details,
        error: None,
        instructions: None,
        metadata: None,
        parallel_tool_calls: None,
        previous_response_id: None,
        reasoning: None,
        store: None,
        temperature: None,
        text: None,
        tool_choice: None,
        tools: Vec::new(),
        top_p: None,
        truncation: None,
        extra: Map::new(),
    };

    Ok(Transcoded {
        body: serde_json::to_vec(&out)?,
        report,
    })
}

fn extract_custom_input(arguments: &str) -> Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(object)) => object
            .get("input")
            .cloned()
            .unwrap_or_else(|| Value::String(trimmed.to_string())),
        Ok(value) => value,
        Err(_) => Value::String(trimmed.to_string()),
    }
}

fn empty_output() -> ResponsesOutput {
    ResponsesOutput {
        type_: String::new(),
        id: None,
        role: None,
        content: Vec::new(),
        status: None,
        encrypted_content: None,
        summary: Vec::new(),
        call_id: None,
        name: None,
        arguments: None,
        namespace: None,
        input: None,
        action: None,
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
