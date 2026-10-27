//! 流式差分：用 sub2api 的 stream converter 作为 oracle，比较事件语义。
//!
//! 参考文件由 sub2api `dba770d56` 生成，映射到本项目的方向命名：
//!
//! | 本项目方向 | sub2api 函数 |
//! |---|---|
//! | `chat_to_anthropic` | `AnthropicEventToResponsesEvents` + `ResponsesEventToChatChunks`（两跳） |
//! | `anthropic_to_chat` | `ChatCompletionsChunkToAnthropicEvents` |
//! | `responses_to_chat` | `ChatCompletionsChunkToResponsesEvents` |
//! | `chat_to_responses` | `ResponsesEventToChatChunks` |
//! | `anthropic_to_responses` | `ResponsesEventToAnthropicEvents` |
//! | `responses_to_anthropic` | `AnthropicEventToResponsesEvents` |
//!
//! 只比较文本、工具调用数、终止原因；不比较完整事件 JSON。

use std::collections::BTreeSet;

use serde_json::Value;
use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, stream_translator};
use solidway_transcode::report::TranscodePolicy;

const CHAT_STREAM: &str = include_str!("fixtures/chat_stream.txt");
const RESPONSES_STREAM: &str = include_str!("fixtures/responses_stream.txt");
const ANTHROPIC_STREAM: &str = include_str!("fixtures/anthropic_stream.txt");

const REF_CHAT_TO_ANTHROPIC: &str = include_str!("reference/chat_to_anthropic_stream.jsonl");
const REF_ANTHROPIC_TO_CHAT: &str = include_str!("reference/anthropic_to_chat_stream.jsonl");
const REF_RESPONSES_TO_CHAT: &str = include_str!("reference/responses_to_chat_stream.jsonl");
const REF_CHAT_TO_RESPONSES: &str = include_str!("reference/chat_to_responses_stream.jsonl");
const REF_ANTHROPIC_TO_RESPONSES: &str =
    include_str!("reference/anthropic_to_responses_stream.jsonl");
const REF_RESPONSES_TO_ANTHROPIC: &str =
    include_str!("reference/responses_to_anthropic_stream.jsonl");

#[derive(Debug, PartialEq, Eq)]
struct Summary {
    text: String,
    tools: usize,
    finish: String,
}

fn sse(jsonl: &str) -> String {
    jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("data: {line}\n\n"))
        .collect()
}

fn parse_sse(text: &str) -> Vec<Value> {
    text.split("\n\n")
        .filter_map(|block| {
            let data = block
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|line| line.trim_start())
                .collect::<Vec<_>>()
                .join("\n");
            if data.is_empty() || data == "[DONE]" {
                return None;
            }
            serde_json::from_str(&data).ok()
        })
        .collect()
}

fn parse_reference(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn ours(src: ProtocolId, dst: ProtocolId, fixture: &str) -> Vec<Value> {
    let plan = Plan {
        src,
        dst,
        policy: TranscodePolicy::BestEffort,
    };
    let mut translator = stream_translator(src, dst, &plan).unwrap();
    let mut out = translator.feed(sse(fixture).as_bytes()).unwrap();
    out.extend(translator.finish().unwrap());
    parse_sse(&String::from_utf8(out).unwrap())
}

fn summary(target: ProtocolId, events: &[Value]) -> Summary {
    match target {
        ProtocolId::OPENAI_CHAT => summary_chat(events),
        ProtocolId::ANTHROPIC_MESSAGES => summary_anthropic(events),
        ProtocolId::OPENAI_RESPONSES => summary_responses(events),
        other => panic!("unsupported target {other}"),
    }
}

fn summary_chat(events: &[Value]) -> Summary {
    let mut text = String::new();
    let mut tools = BTreeSet::new();
    let mut finish = String::new();
    for event in events {
        let Some(choices) = event.get("choices").and_then(Value::as_array) else {
            continue;
        };
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(content) = delta.get("content").and_then(Value::as_str) {
                    text.push_str(content);
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        if let Some(index) = call.get("index").and_then(Value::as_u64) {
                            tools.insert(index);
                        }
                    }
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                finish = reason.to_string();
            }
        }
    }
    Summary {
        text,
        tools: tools.len(),
        finish,
    }
}

fn summary_anthropic(events: &[Value]) -> Summary {
    let mut text = String::new();
    let mut tools = 0;
    let mut finish = String::new();
    for event in events {
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                if event
                    .get("content_block")
                    .and_then(|block| block.get("type"))
                    .and_then(Value::as_str)
                    == Some("tool_use")
                {
                    tools += 1;
                }
            }
            Some("content_block_delta") => {
                if event
                    .get("delta")
                    .and_then(|delta| delta.get("type"))
                    .and_then(Value::as_str)
                    == Some("text_delta")
                    && let Some(delta) = event
                        .get("delta")
                        .and_then(|delta| delta.get("text"))
                        .and_then(Value::as_str)
                {
                    text.push_str(delta);
                }
            }
            Some("message_delta") => {
                if let Some(reason) = event
                    .get("delta")
                    .and_then(|delta| delta.get("stop_reason"))
                    .and_then(Value::as_str)
                {
                    finish = reason.to_string();
                }
            }
            _ => {}
        }
    }
    Summary {
        text,
        tools,
        finish,
    }
}

fn summary_responses(events: &[Value]) -> Summary {
    let mut text = String::new();
    let mut tools = 0;
    let mut finish = String::new();
    for event in events {
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_text.delta") => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    text.push_str(delta);
                }
            }
            Some("response.output_item.added") => {
                if event
                    .get("item")
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    == Some("function_call")
                {
                    tools += 1;
                }
            }
            Some("response.completed") | Some("response.incomplete") => {
                if let Some(status) = event
                    .get("response")
                    .and_then(|response| response.get("status"))
                    .and_then(Value::as_str)
                {
                    finish = status.to_string();
                }
            }
            _ => {}
        }
    }
    Summary {
        text,
        tools,
        finish,
    }
}

fn assert_matches(
    src: ProtocolId,
    dst: ProtocolId,
    target: ProtocolId,
    fixture: &str,
    reference: &str,
) {
    let ours = summary(target, &ours(src, dst, fixture));
    let reference = summary(target, &parse_reference(reference));
    assert_eq!(
        ours, reference,
        "stream differential mismatch for {src} -> {dst}"
    );
}

#[test]
fn streaming_differential() {
    assert_matches(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        ANTHROPIC_STREAM,
        REF_CHAT_TO_ANTHROPIC,
    );
    assert_matches(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        CHAT_STREAM,
        REF_ANTHROPIC_TO_CHAT,
    );
    assert_matches(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        CHAT_STREAM,
        REF_RESPONSES_TO_CHAT,
    );
    assert_matches(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        RESPONSES_STREAM,
        REF_CHAT_TO_RESPONSES,
    );
    assert_matches(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        RESPONSES_STREAM,
        REF_ANTHROPIC_TO_RESPONSES,
    );
    assert_matches(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        ANTHROPIC_STREAM,
        REF_RESPONSES_TO_ANTHROPIC,
    );
}
