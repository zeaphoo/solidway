//! Anthropic content -> Chat content 的机械转换。

use serde_json::Map;

use crate::registry::Plan;
use crate::report::{Error, Report, Side};
use crate::wire::anthropic::{AnthropicContentBlock, AnthropicToolResultContent};
use crate::wire::openai_chat::{ChatContent, ChatContentPart, ChatFile, ChatImageUrl, ChatMessage};

pub(super) fn empty_message() -> ChatMessage {
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

pub(super) fn content_block_to_chat_part(
    block: &AnthropicContentBlock,
    report: &mut Report,
    plan: &Plan,
    pointer: &str,
) -> Result<Option<ChatContentPart>, Error> {
    match block.type_.as_str() {
        "text" => Ok(Some(ChatContentPart {
            type_: "text".to_string(),
            text: block.text.clone(),
            ..empty_part()
        })),
        "image" => {
            let Some(source) = block.source.as_ref() else {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("{pointer}/source"),
                    "Anthropic image block is missing source",
                )?;
                return Ok(None);
            };
            let url = match source.type_.as_str() {
                "base64" => format!(
                    "data:{};base64,{}",
                    source.media_type.as_deref().unwrap_or("image/png"),
                    source.data.as_deref().unwrap_or_default()
                ),
                "url" => source.url.clone().unwrap_or_default(),
                other => {
                    report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!("{pointer}/source/type"),
                        format!("Anthropic image source {other} has no Chat equivalent"),
                    )?;
                    return Ok(None);
                }
            };
            Ok(Some(ChatContentPart {
                type_: "image_url".to_string(),
                image_url: Some(ChatImageUrl {
                    url,
                    detail: None,
                    extra: Map::new(),
                }),
                ..empty_part()
            }))
        }
        "document" => {
            let Some(source) = block.source.as_ref() else {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    format!("{pointer}/source"),
                    "Anthropic document block is missing source",
                )?;
                return Ok(None);
            };
            if source.type_ == "text" {
                report.degraded(
                    Side::Target,
                    format!("{pointer}/source"),
                    "Anthropic text document is emitted as Chat text",
                );
                return Ok(Some(ChatContentPart {
                    type_: "text".to_string(),
                    text: source.data.clone(),
                    ..empty_part()
                }));
            }
            let file = match source.type_.as_str() {
                "base64" => Some(ChatFile {
                    filename: None,
                    file_data: Some(format!(
                        "data:{};base64,{}",
                        source.media_type.as_deref().unwrap_or("application/pdf"),
                        source.data.as_deref().unwrap_or_default()
                    )),
                    file_id: None,
                    extra: Map::new(),
                }),
                "file" => Some(ChatFile {
                    filename: None,
                    file_data: None,
                    file_id: source.file_id.clone(),
                    extra: Map::new(),
                }),
                _ => None,
            };
            match file {
                Some(file) => Ok(Some(ChatContentPart {
                    type_: "file".to_string(),
                    file: Some(file),
                    ..empty_part()
                })),
                None => {
                    report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!("{pointer}/source/type"),
                        "Anthropic document source has no Chat equivalent",
                    )?;
                    Ok(None)
                }
            }
        }
        "server_tool_use" | "web_search_tool_result" | "web_fetch_tool_result" => {
            report.unsupported(
                plan.policy,
                Side::Source,
                format!("{pointer}/type"),
                format!(
                    "Anthropic server tool block {} has no Chat equivalent",
                    block.type_
                ),
            )?;
            Ok(None)
        }
        other => {
            report.unsupported(
                plan.policy,
                Side::Source,
                format!("{pointer}/type"),
                format!("Anthropic content block {other} has no Chat equivalent"),
            )?;
            Ok(None)
        }
    }
}

pub(super) fn tool_result_messages(
    block: &AnthropicContentBlock,
    report: &mut Report,
    plan: &Plan,
    message_index: usize,
    block_index: usize,
) -> Result<Vec<ChatMessage>, Error> {
    let mut text_parts = Vec::new();
    let mut media_parts = Vec::new();
    let mut output = None;

    match block.content.as_ref() {
        Some(AnthropicToolResultContent::Text(text)) => output = Some(text.clone()),
        Some(AnthropicToolResultContent::Blocks(blocks)) => {
            for (part_index, part) in blocks.iter().enumerate() {
                match part.type_.as_str() {
                    "text" => {
                        if let Some(text) = part.text.as_deref().filter(|text| !text.is_empty()) {
                            text_parts.push(ChatContentPart {
                                type_: "text".to_string(),
                                text: Some(text.to_string()),
                                ..empty_part()
                            });
                        }
                    }
                    "image" | "document" => {
                        if let Some(part) = content_block_to_chat_part(
                            part,
                            report,
                            plan,
                            &format!(
                                "/messages/{message_index}/content/{block_index}/content/{part_index}"
                            ),
                        )? {
                            media_parts.push(part);
                        }
                    }
                    other => report.unsupported(
                        plan.policy,
                        Side::Source,
                        format!(
                            "/messages/{message_index}/content/{block_index}/content/{part_index}/type"
                        ),
                        format!("tool_result block {other} has no Chat equivalent"),
                    )?,
                }
            }
        }
        Some(AnthropicToolResultContent::Other(value)) => output = Some(value.to_string()),
        None => {}
    }

    if block.tool_use_id.is_none() {
        report.degraded(
            Side::Target,
            format!("/messages/{message_index}/content/{block_index}/tool_use_id"),
            "Anthropic tool_result has no tool_use_id; Chat tool message is emitted without tool_call_id",
        );
    }

    let content = if let Some(text) = output {
        ChatContent::Text(text)
    } else if text_parts.len() == 1 && text_parts[0].type_ == "text" {
        ChatContent::Text(text_parts[0].text.clone().unwrap_or_default())
    } else if text_parts.is_empty() {
        ChatContent::Text(String::new())
    } else {
        ChatContent::Parts(text_parts)
    };

    let mut messages = vec![ChatMessage {
        role: "tool".to_string(),
        content: Some(content),
        tool_call_id: block.tool_use_id.clone(),
        name: block.name.clone(),
        ..empty_message()
    }];

    if !media_parts.is_empty() {
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: Some(ChatContent::Parts(media_parts)),
            ..empty_message()
        });
        report.degraded(
            Side::Target,
            format!("/messages/{message_index}/content/{block_index}/content"),
            "tool_result media is lifted into a separate user message",
        );
    }

    Ok(messages)
}

pub(super) fn join_text_blocks(blocks: &[AnthropicContentBlock]) -> String {
    blocks
        .iter()
        .filter(|block| block.type_ == "text")
        .filter_map(|block| block.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn empty_part() -> ChatContentPart {
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
