//! Responses content -> Anthropic content 的机械转换。

use serde_json::{Map, Value};

use crate::registry::Plan;
use crate::report::{Error, Report, Side};
use crate::wire::anthropic::{AnthropicContentBlock, AnthropicSource};
use crate::wire::openai_responses::{ResponsesContent, ResponsesImageUrl};

pub(super) fn content_to_blocks(
    content: Option<&ResponsesContent>,
    assistant: bool,
    report: &mut Report,
    plan: &Plan,
    pointer: &str,
) -> Result<Vec<AnthropicContentBlock>, Error> {
    match content {
        None => Ok(Vec::new()),
        Some(ResponsesContent::Text(text)) => Ok(vec![AnthropicContentBlock {
            type_: "text".to_string(),
            text: Some(text.clone()),
            ..empty_block()
        }]),
        Some(ResponsesContent::Parts(parts)) => {
            let mut out = Vec::new();
            for (index, part) in parts.iter().enumerate() {
                match part.type_.as_str() {
                    "input_text" | "output_text" | "text" => out.push(AnthropicContentBlock {
                        type_: "text".to_string(),
                        text: part.text.clone(),
                        ..empty_block()
                    }),
                    "input_image" => {
                        if let Some(image) = part.image_url.as_ref() {
                            let url = match image {
                                ResponsesImageUrl::Url(url) => url.clone(),
                                ResponsesImageUrl::Object(object) => object.url.clone(),
                            };
                            out.push(AnthropicContentBlock {
                                type_: "image".to_string(),
                                source: Some(image_source(&url)),
                                ..empty_block()
                            });
                        }
                    }
                    "input_file" => {
                        if part.file_data.is_none() {
                            report.degraded(
                                Side::Source,
                                format!("{pointer}/{index}"),
                                "input_file without file_data has no Anthropic document source",
                            );
                            continue;
                        }
                        out.push(AnthropicContentBlock {
                            type_: "document".to_string(),
                            source: Some(file_source(
                                part.file_data.as_deref(),
                                part.file_id.as_deref(),
                            )),
                            ..empty_block()
                        });
                    }
                    "refusal" => {
                        if !assistant
                            && let Some(text) = part.refusal.clone().filter(|text| !text.is_empty())
                        {
                            out.push(AnthropicContentBlock {
                                type_: "text".to_string(),
                                text: Some(text),
                                ..empty_block()
                            });
                        }
                    }
                    other => report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!("{pointer}/{index}/type"),
                        format!("Responses content part {other} has no Anthropic equivalent"),
                    )?,
                }
            }
            Ok(out)
        }
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

fn file_source(file_data: Option<&str>, file_id: Option<&str>) -> AnthropicSource {
    if let Some(data) = file_data
        && let Some(rest) = data.strip_prefix("data:")
        && let Some((meta, payload)) = rest.split_once(',')
    {
        return AnthropicSource {
            type_: "base64".to_string(),
            media_type: Some(meta.strip_suffix(";base64").unwrap_or(meta).to_string()),
            data: Some(payload.to_string()),
            url: None,
            file_id: None,
            extra: Map::new(),
        };
    }
    AnthropicSource {
        type_: "file".to_string(),
        media_type: None,
        data: None,
        url: None,
        file_id: file_id.map(ToString::to_string),
        extra: Map::new(),
    }
}

pub(super) fn empty_block() -> AnthropicContentBlock {
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

pub(super) fn decode_thinking(encrypted: &str) -> Option<Value> {
    if let Some(json) = encrypted.strip_prefix("anthropic-thinking-v1:") {
        return serde_json::from_str(json).ok();
    }
    if encrypted.is_empty() {
        None
    } else {
        Some(serde_json::json!({"signature": encrypted}))
    }
}
