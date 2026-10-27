//! 与 sub2api 的差分对照（请求方向）。
//!
//! 参考输出由 sub2api `dba770d56` 的 `apicompat` 包生成：
//!
//! ```txt
//! go run ./cmd/transcode-oracle <direction> <fixture>
//! ```
//!
//! 只比较结构不变量（工具调用 / 工具结果 / 关键文本），不比较完整 JSON：
//! 两边对 thinking 的表示、默认值、min-token floor 等存在已知差异。

use serde_json::Value;
use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, Transcoded, direction};
use solidway_transcode::report::TranscodePolicy;

const PARALLEL_CHAT: &str = include_str!("fixtures/parallel_tools_chat_request.json");
const PARALLEL_ANTHROPIC: &str = include_str!("fixtures/parallel_tools_anthropic_request.json");
const PARALLEL_RESPONSES: &str = include_str!("fixtures/parallel_tools_responses_request.json");

const REF_CHAT_TO_ANTHROPIC: &str = include_str!("reference/chat_to_anthropic_request.json");
const REF_CHAT_TO_RESPONSES: &str = include_str!("reference/chat_to_responses_request.json");
const REF_ANTHROPIC_TO_CHAT: &str = include_str!("reference/anthropic_to_chat_request.json");
const REF_ANTHROPIC_TO_RESPONSES: &str =
    include_str!("reference/anthropic_to_responses_request.json");
const REF_RESPONSES_TO_CHAT: &str = include_str!("reference/responses_to_chat_request.json");
const REF_RESPONSES_TO_ANTHROPIC: &str =
    include_str!("reference/responses_to_anthropic_request.json");

#[derive(Debug, PartialEq, Eq)]
struct Summary {
    tool_calls: usize,
    tool_results: usize,
    has_text: bool,
}

fn ours(src: ProtocolId, dst: ProtocolId, body: &str) -> Value {
    let plan = Plan {
        src,
        dst,
        policy: TranscodePolicy::BestEffort,
    };
    let direction = direction(src, dst).expect("direction");
    let Transcoded { body, .. } = direction.request(body.as_bytes(), &plan).unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn summary_for(target: ProtocolId, value: &Value) -> Summary {
    match target {
        ProtocolId::OPENAI_CHAT => summary_chat(value),
        ProtocolId::ANTHROPIC_MESSAGES => summary_anthropic(value),
        ProtocolId::OPENAI_RESPONSES => summary_responses(value),
        other => panic!("unsupported target {other}"),
    }
}

fn summary_chat(value: &Value) -> Summary {
    let mut summary = Summary {
        tool_calls: 0,
        tool_results: 0,
        has_text: false,
    };
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return summary;
    };
    for message in messages {
        summary.tool_calls += message
            .get("tool_calls")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if message.get("role").and_then(Value::as_str) == Some("tool") {
            summary.tool_results += 1;
        }
        if text_contains(message.get("content"), "Checking both.") {
            summary.has_text = true;
        }
    }
    summary
}

fn summary_anthropic(value: &Value) -> Summary {
    let mut summary = Summary {
        tool_calls: 0,
        tool_results: 0,
        has_text: false,
    };
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return summary;
    };
    for message in messages {
        let Some(content) = message.get("content") else {
            continue;
        };
        if text_contains(Some(content), "Checking both.") {
            summary.has_text = true;
        }
        let Some(blocks) = content.as_array() else {
            continue;
        };
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => summary.tool_calls += 1,
                Some("tool_result") => summary.tool_results += 1,
                _ => {}
            }
        }
    }
    summary
}

fn summary_responses(value: &Value) -> Summary {
    let mut summary = Summary {
        tool_calls: 0,
        tool_results: 0,
        has_text: false,
    };
    let Some(input) = value.get("input").and_then(Value::as_array) else {
        return summary;
    };
    for item in input {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => summary.tool_calls += 1,
            Some("function_call_output") => summary.tool_results += 1,
            Some("message") if text_contains(item.get("content"), "Checking both.") => {
                summary.has_text = true;
            }
            _ => {}
        }
    }
    summary
}

fn text_contains(value: Option<&Value>, needle: &str) -> bool {
    match value {
        Some(Value::String(text)) => text.contains(needle),
        Some(Value::Array(parts)) => parts.iter().any(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains(needle))
        }),
        _ => false,
    }
}

fn assert_matches_reference(src: ProtocolId, dst: ProtocolId, fixture: &str, reference: &str) {
    let ours = ours(src, dst, fixture);
    let reference: Value = serde_json::from_str(reference).unwrap();
    let ours_summary = summary_for(dst, &ours);
    let reference_summary = summary_for(dst, &reference);
    assert_eq!(
        ours_summary, reference_summary,
        "differential mismatch for {src} -> {dst}\nours={ours}\nreference={reference}"
    );
}

#[test]
fn differential_parallel_tools_requests() {
    assert_matches_reference(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        PARALLEL_CHAT,
        REF_CHAT_TO_ANTHROPIC,
    );
    assert_matches_reference(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        PARALLEL_CHAT,
        REF_CHAT_TO_RESPONSES,
    );
    assert_matches_reference(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        PARALLEL_ANTHROPIC,
        REF_ANTHROPIC_TO_CHAT,
    );
    assert_matches_reference(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        PARALLEL_ANTHROPIC,
        REF_ANTHROPIC_TO_RESPONSES,
    );
    assert_matches_reference(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        PARALLEL_RESPONSES,
        REF_RESPONSES_TO_CHAT,
    );
    assert_matches_reference(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        PARALLEL_RESPONSES,
        REF_RESPONSES_TO_ANTHROPIC,
    );
}
