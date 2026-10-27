//! Chat content -> Anthropic content 的机械转换。

use serde_json::{Map, Value, json};

use crate::registry::Plan;
use crate::report::{Error, Report, Side};
use crate::wire::anthropic::{
    AnthropicCacheControl, AnthropicContentBlock, AnthropicSource, AnthropicToolResultContent,
};
use crate::wire::openai_chat::{ChatContent, ChatContentPart, ChatMessage};

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

pub(super) fn append_chat_content(
    content: Option<&ChatContent>,
    blocks: &mut Vec<AnthropicContentBlock>,
    report: &mut Report,
    plan: &Plan,
    pointer: &str,
) -> Result<(), Error> {
    match content {
        None => Ok(()),
        Some(ChatContent::Text(text)) => {
            if !text.is_empty() {
                blocks.push(AnthropicContentBlock {
                    type_: "text".to_string(),
                    text: Some(text.clone()),
                    ..empty_block()
                });
            }
            Ok(())
        }
        Some(ChatContent::Parts(parts)) => {
            for (index, part) in parts.iter().enumerate() {
                append_chat_part(part, blocks, report, plan, &format!("{pointer}/{index}"))?;
            }
            Ok(())
        }
    }
}

fn append_chat_part(
    part: &ChatContentPart,
    blocks: &mut Vec<AnthropicContentBlock>,
    report: &mut Report,
    plan: &Plan,
    pointer: &str,
) -> Result<(), Error> {
    let cache_control = part
        .prompt_cache_breakpoint
        .as_ref()
        .map(|_| AnthropicCacheControl {
            type_: "ephemeral".to_string(),
            ttl: None,
            extra: Map::new(),
        });

    match part.type_.as_str() {
        "text" => {
            if let Some(text) = part.text.as_deref().filter(|text| !text.is_empty()) {
                blocks.push(AnthropicContentBlock {
                    type_: "text".to_string(),
                    text: Some(text.to_string()),
                    cache_control,
                    ..empty_block()
                });
            }
        }
        "image_url" => {
            let Some(image) = part.image_url.as_ref() else {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("{pointer}/image_url"),
                    "image content part is missing image_url",
                )?;
                return Ok(());
            };
            let source = match data_uri_parts(&image.url) {
                Some((media_type, data)) => AnthropicSource {
                    type_: "base64".to_string(),
                    media_type: Some(media_type),
                    data: Some(data),
                    url: None,
                    file_id: None,
                    extra: Map::new(),
                },
                None => AnthropicSource {
                    type_: "url".to_string(),
                    media_type: None,
                    data: None,
                    url: Some(image.url.clone()),
                    file_id: None,
                    extra: Map::new(),
                },
            };
            blocks.push(AnthropicContentBlock {
                type_: "image".to_string(),
                source: Some(source),
                cache_control,
                ..empty_block()
            });
        }
        "file" => {
            let Some(file) = part.file.as_ref() else {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("{pointer}/file"),
                    "file content part is missing file",
                )?;
                return Ok(());
            };
            let source = if let Some((media_type, data)) =
                file.file_data.as_deref().and_then(data_uri_parts)
            {
                AnthropicSource {
                    type_: "base64".to_string(),
                    media_type: Some(media_type),
                    data: Some(data),
                    url: None,
                    file_id: None,
                    extra: Map::new(),
                }
            } else if let Some(file_id) = file.file_id.clone() {
                AnthropicSource {
                    type_: "file".to_string(),
                    media_type: None,
                    data: None,
                    url: None,
                    file_id: Some(file_id),
                    extra: Map::new(),
                }
            } else {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("{pointer}/file"),
                    "Chat file has neither file_data nor file_id",
                )?;
                return Ok(());
            };
            blocks.push(AnthropicContentBlock {
                type_: "document".to_string(),
                source: Some(source),
                cache_control,
                ..empty_block()
            });
        }
        "input_audio" => {
            report.unsupported(
                plan.policy,
                Side::Source,
                format!("{pointer}/type"),
                "Anthropic Messages has no audio input content block",
            )?;
        }
        "refusal" => {
            if let Some(text) = part.refusal.as_deref().filter(|text| !text.is_empty()) {
                blocks.push(AnthropicContentBlock {
                    type_: "text".to_string(),
                    text: Some(text.to_string()),
                    cache_control,
                    ..empty_block()
                });
                report.degraded(
                    Side::Target,
                    pointer,
                    "Chat refusal content is emitted as an Anthropic text block",
                );
            }
        }
        other => {
            report.unsupported(
                plan.policy,
                Side::Source,
                format!("{pointer}/type"),
                format!("unknown Chat content type: {other}"),
            )?;
        }
    }
    Ok(())
}

pub(super) fn tool_result_block(
    message: &ChatMessage,
    report: &mut Report,
    plan: &Plan,
    index: usize,
) -> Result<AnthropicContentBlock, Error> {
    let tool_use_id = message
        .tool_call_id
        .clone()
        .or_else(|| message.name.clone());
    if tool_use_id.is_none() {
        report.degraded(
            Side::Target,
            format!("/messages/{index}/tool_call_id"),
            "Chat tool message has no tool_call_id; Anthropic tool_result is emitted without tool_use_id",
        );
    }
    let content = match message.content.as_ref() {
        Some(ChatContent::Text(text)) => AnthropicToolResultContent::Text(text.clone()),
        Some(ChatContent::Parts(parts)) => {
            let mut blocks = Vec::new();
            append_chat_content(
                Some(&ChatContent::Parts(parts.clone())),
                &mut blocks,
                report,
                plan,
                &format!("/messages/{index}/content"),
            )?;
            AnthropicToolResultContent::Blocks(blocks)
        }
        None => AnthropicToolResultContent::Text(String::new()),
    };
    let is_error = message
        .extra
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Ok(AnthropicContentBlock {
        type_: "tool_result".to_string(),
        tool_use_id,
        content: Some(content),
        is_error: Some(is_error),
        ..empty_block()
    })
}

pub(super) fn parse_tool_arguments(
    arguments: &str,
    report: &mut Report,
    plan: &Plan,
    pointer: &str,
) -> Result<Value, Error> {
    if arguments.trim().is_empty() {
        return Ok(json!({}));
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(Value::Object(value)) => Ok(Value::Object(value)),
        Ok(_) => {
            report.unsupported(
                plan.policy,
                Side::Target,
                pointer,
                "Anthropic tool_use.input must be an object",
            )?;
            Ok(json!({}))
        }
        Err(err) => {
            report.unsupported(
                plan.policy,
                Side::Target,
                pointer,
                format!("Chat tool arguments are not valid JSON: {err}"),
            )?;
            Ok(json!({}))
        }
    }
}

fn data_uri_parts(value: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let media_type = meta.strip_suffix(";base64").unwrap_or(meta).to_string();
    Some((media_type, data.to_string()))
}
