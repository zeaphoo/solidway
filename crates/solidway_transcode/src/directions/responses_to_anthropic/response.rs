//! Anthropic response -> Responses response。

use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::common::empty;
use crate::common::usage::{positive, saturating_sum};
use crate::registry::{Plan, Transcoded};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{
    AnthropicContentBlock, AnthropicError, AnthropicResponse, AnthropicToolResultContent,
};
use crate::wire::openai_responses::{
    ResponsesContentPart, ResponsesError, ResponsesErrorEnvelope, ResponsesIncompleteDetails,
    ResponsesInputTokensDetails, ResponsesOutput, ResponsesResponse, ResponsesSummary,
    ResponsesUsage,
};

pub(super) fn convert(body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
    if empty::is_empty_body(body) {
        return Ok(Transcoded {
            body: serde_json::to_vec(&empty::responses_response())?,
            report: Report::default(),
        });
    }

    let value: Value = serde_json::from_slice(body)?;

    if value.get("type").and_then(Value::as_str) == Some("error") {
        let err: AnthropicError = serde_json::from_value(value)?;
        return Ok(Transcoded {
            body: serde_json::to_vec(&ResponsesErrorEnvelope {
                error: ResponsesError {
                    code: Some(err.error.type_.clone()),
                    message: err.error.message,
                    param: None,
                    type_: Some(map_error_type(&err.error.type_)),
                    extra: err.error.extra,
                },
            })?,
            report: Report::default(),
        });
    }

    let resp: AnthropicResponse = serde_json::from_value(value)?;
    let mut report = Report::default();
    if resp.stop_sequence.is_some() {
        report.degraded(
            Side::Source,
            "/stop_sequence",
            "Responses has no stop_sequence field",
        );
    }
    if resp.container.is_some() {
        report.degraded(
            Side::Source,
            "/container",
            "Responses has no container field",
        );
    }
    for key in resp.extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("/{key}"),
            "unknown Anthropic response field is not mapped to Responses",
        );
    }
    if let Some(usage) = resp.usage.as_ref() {
        if usage.cache_creation.is_some() {
            report.degraded(
                Side::Source,
                "/usage/cache_creation",
                "Responses usage has no cache_creation breakdown",
            );
        }
        if usage.server_tool_use.is_some() {
            report.degraded(
                Side::Source,
                "/usage/server_tool_use",
                "Responses usage has no server_tool_use breakdown",
            );
        }
    }

    let mut output = Vec::new();
    let mut server_tools: HashMap<String, usize> = HashMap::new();

    for (index, block) in resp.content.iter().enumerate() {
        if block.cache_control.is_some() {
            report.degraded(
                Side::Source,
                format!("/content/{index}/cache_control"),
                "Responses has no cache_control field",
            );
        }
        for key in block.extra.keys() {
            report.warn(
                WarningCode::UnknownField,
                Side::Source,
                format!("/content/{index}/{key}"),
                "unknown Anthropic content field is not mapped to Responses",
            );
        }
        match block.type_.as_str() {
            "thinking" | "redacted_thinking" => {
                output.push(ResponsesOutput {
                    type_: "reasoning".to_string(),
                    id: Some(format!("{}-reasoning-{index}", resp.id)),
                    encrypted_content: Some(encode_thinking(block)),
                    summary: vec![ResponsesSummary {
                        type_: "summary_text".to_string(),
                        text: block.thinking.clone().unwrap_or_default(),
                        extra: Map::new(),
                    }],
                    ..empty_output()
                });
            }
            "text" => {
                if let Some(text) = block.text.as_deref().filter(|text| !text.is_empty()) {
                    output.push(ResponsesOutput {
                        type_: "message".to_string(),
                        id: Some(format!("{}-message-{index}", resp.id)),
                        role: Some("assistant".to_string()),
                        status: Some("completed".to_string()),
                        content: vec![ResponsesContentPart {
                            type_: "output_text".to_string(),
                            text: Some(text.to_string()),
                            annotations: block.citations.clone(),
                            ..empty_part()
                        }],
                        ..empty_output()
                    });
                }
            }
            "server_tool_use" => {
                let name = block.name.as_deref().unwrap_or("web_search");
                let type_ = match name {
                    "web_search" => "web_search_call".to_string(),
                    "x_search" => "x_search_call".to_string(),
                    other => format!("{other}_call"),
                };
                let id = block
                    .id
                    .clone()
                    .unwrap_or_else(|| format!("{}-{name}-{index}", resp.id));
                output.push(ResponsesOutput {
                    type_,
                    id: Some(id.clone()),
                    status: Some("completed".to_string()),
                    action: Some(anthropic_input_to_action(block.input.as_ref())),
                    ..empty_output()
                });
                server_tools.insert(id, output.len() - 1);
            }
            result if result.ends_with("_tool_result") => {
                let name = result.trim_end_matches("_tool_result");
                let call_type = format!("{name}_call");
                let tool_use_id = block
                    .tool_use_id
                    .clone()
                    .or_else(|| block.id.clone())
                    .unwrap_or_else(|| format!("{}-{name}", resp.id));
                let results = anthropic_server_tool_results(block);
                if let Some(position) = server_tools.get(&tool_use_id).copied() {
                    output[position]
                        .extra
                        .insert("results".to_string(), results);
                    if !block.citations.is_empty() {
                        output[position].extra.insert(
                            "citations".to_string(),
                            Value::Array(block.citations.clone()),
                        );
                    }
                } else {
                    let mut extra = Map::new();
                    extra.insert("results".to_string(), results);
                    if !block.citations.is_empty() {
                        extra.insert(
                            "citations".to_string(),
                            Value::Array(block.citations.clone()),
                        );
                    }
                    output.push(ResponsesOutput {
                        type_: call_type,
                        id: Some(tool_use_id),
                        status: Some("completed".to_string()),
                        extra,
                        ..empty_output()
                    });
                }
            }
            "tool_use" => {
                output.push(ResponsesOutput {
                    type_: "function_call".to_string(),
                    id: Some(format!("{}-call-{index}", resp.id)),
                    call_id: block.id.clone(),
                    name: block.name.clone(),
                    arguments: Some(Value::String(serde_json::to_string(
                        &block.input.clone().unwrap_or_else(|| json!({})),
                    )?)),
                    status: Some("completed".to_string()),
                    ..empty_output()
                });
            }
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Target,
                    format!("/content/{index}/type"),
                    format!("Anthropic content block {other} has no Responses equivalent"),
                )?;
            }
        }
    }

    let (status, incomplete_details) = match resp.stop_reason.as_deref() {
        Some("max_tokens") => (
            "incomplete".to_string(),
            Some(ResponsesIncompleteDetails {
                reason: "max_output_tokens".to_string(),
                extra: Map::new(),
            }),
        ),
        Some("refusal") => (
            "incomplete".to_string(),
            Some(ResponsesIncompleteDetails {
                reason: "content_filter".to_string(),
                extra: Map::new(),
            }),
        ),
        _ => ("completed".to_string(), None),
    };

    let usage = resp.usage.as_ref().map(|usage| {
        let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
        let cache_creation = usage.cache_creation_input_tokens.unwrap_or(0);
        let input = saturating_sum(&[usage.input_tokens, Some(cache_read), Some(cache_creation)]);
        let output = usage.output_tokens.unwrap_or(0);
        ResponsesUsage {
            input_tokens: Some(input),
            output_tokens: Some(output),
            total_tokens: Some(saturating_sum(&[Some(input), Some(output)])),
            cache_creation_input_tokens: positive(cache_creation),
            input_tokens_details: positive(cache_read).map(|cache_read| {
                ResponsesInputTokensDetails {
                    cached_tokens: Some(cache_read),
                    ..empty_input_details()
                }
            }),
            output_tokens_details: None,
            extra: Map::new(),
        }
    });

    report.warn(
        WarningCode::Synthesized,
        Side::Target,
        "/created_at",
        "Anthropic response has no created_at; using 0",
    );

    let id = if resp.id.is_empty() {
        "resp_transcoded".to_string()
    } else {
        resp.id
    };

    let out = ResponsesResponse {
        id,
        object: "response".to_string(),
        created_at: 0,
        model: resp.model,
        status,
        output,
        usage,
        service_tier: resp
            .usage
            .as_ref()
            .and_then(|usage| usage.service_tier.clone()),
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

fn anthropic_input_to_action(input: Option<&Value>) -> Value {
    match input {
        Some(Value::Object(object)) => {
            if let Some(query) = object.get("query").and_then(Value::as_str) {
                json!({"type": "search", "query": query})
            } else {
                Value::Object(object.clone())
            }
        }
        Some(Value::String(query)) => json!({"type": "search", "query": query}),
        Some(other) => other.clone(),
        None => json!({}),
    }
}

fn anthropic_server_tool_results(block: &AnthropicContentBlock) -> Value {
    let value = match block.content.as_ref() {
        Some(AnthropicToolResultContent::Other(value)) => value.clone(),
        Some(AnthropicToolResultContent::Text(text)) => json!([{"type": "text", "text": text}]),
        Some(AnthropicToolResultContent::Blocks(blocks)) => {
            serde_json::to_value(blocks).unwrap_or_else(|_| json!([]))
        }
        None => json!([]),
    };
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

fn empty_input_details() -> ResponsesInputTokensDetails {
    ResponsesInputTokensDetails {
        cached_tokens: None,
        audio_tokens: None,
        cache_creation_tokens: None,
        cache_write_tokens: None,
        extra: Map::new(),
    }
}

fn encode_thinking(block: &crate::wire::anthropic::AnthropicContentBlock) -> String {
    block
        .signature
        .clone()
        .or_else(|| block.data.clone())
        .unwrap_or_default()
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
