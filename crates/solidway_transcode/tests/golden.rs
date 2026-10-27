//! 通用 golden fixture harness。
//!
//! 目录结构：
//!
//! ```txt
//! tests/golden/<direction>/<case>/
//!   request.src.json   客户端请求
//!   request.dst.json   上游期望请求
//!   response.dst.json  上游响应
//!   response.src.json  客户端期望响应
//!   expect.json        { "request_warnings": [...], "response_warnings": [...] }
//! ```
//!
//! 请求 / 响应使用 JSON 语义比较（忽略键序）；`expect.json` 只校验 warning pointer。

use std::fs;
use std::path::Path;

use serde_json::Value;
use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, Transcoded, direction};
use solidway_transcode::report::{Report, TranscodePolicy};

fn protocols(direction_name: &str) -> (ProtocolId, ProtocolId) {
    use ProtocolId as P;
    match direction_name {
        "chat_to_anthropic" => (P::OPENAI_CHAT, P::ANTHROPIC_MESSAGES),
        "anthropic_to_chat" => (P::ANTHROPIC_MESSAGES, P::OPENAI_CHAT),
        "chat_to_responses" => (P::OPENAI_CHAT, P::OPENAI_RESPONSES),
        "responses_to_chat" => (P::OPENAI_RESPONSES, P::OPENAI_CHAT),
        "anthropic_to_responses" => (P::ANTHROPIC_MESSAGES, P::OPENAI_RESPONSES),
        "responses_to_anthropic" => (P::OPENAI_RESPONSES, P::ANTHROPIC_MESSAGES),
        "chat_to_codex" => (P::OPENAI_CHAT, P::PRIVATE_CODEX),
        "codex_to_chat" => (P::PRIVATE_CODEX, P::OPENAI_CHAT),
        "claude_code_to_chat" => (P::PRIVATE_CLAUDE_CODE, P::OPENAI_CHAT),
        "chat_to_claude_code" => (P::OPENAI_CHAT, P::PRIVATE_CLAUDE_CODE),
        "codex_to_anthropic" => (P::PRIVATE_CODEX, P::ANTHROPIC_MESSAGES),
        "anthropic_to_codex" => (P::ANTHROPIC_MESSAGES, P::PRIVATE_CODEX),
        "claude_code_to_anthropic" => (P::PRIVATE_CLAUDE_CODE, P::ANTHROPIC_MESSAGES),
        "anthropic_to_claude_code" => (P::ANTHROPIC_MESSAGES, P::PRIVATE_CLAUDE_CODE),
        "codex_to_responses" => (P::PRIVATE_CODEX, P::OPENAI_RESPONSES),
        "responses_to_codex" => (P::OPENAI_RESPONSES, P::PRIVATE_CODEX),
        "claude_code_to_responses" => (P::PRIVATE_CLAUDE_CODE, P::OPENAI_RESPONSES),
        "responses_to_claude_code" => (P::OPENAI_RESPONSES, P::PRIVATE_CLAUDE_CODE),
        "codex_to_claude_code" => (P::PRIVATE_CODEX, P::PRIVATE_CLAUDE_CODE),
        "claude_code_to_codex" => (P::PRIVATE_CLAUDE_CODE, P::PRIVATE_CODEX),
        other => panic!("unknown golden direction: {other}"),
    }
}

fn read_optional(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

fn read_json(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|err| panic!("parse {}: {err}", path.display()))
}

fn expect_list(expect: &Value, key: &str) -> Vec<String> {
    expect
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn assert_warnings(report: &Report, expected: &[String], label: &str) {
    for pointer in expected {
        assert!(
            report
                .warnings
                .iter()
                .any(|warning| warning.pointer == *pointer),
            "{label}: missing warning {pointer}, got {:?}",
            report.warnings
        );
    }
}

fn run_request(src: ProtocolId, dst: ProtocolId, body: &[u8]) -> Transcoded<Vec<u8>> {
    direction(src, dst)
        .expect("direction")
        .request(
            body,
            &Plan {
                src,
                dst,
                policy: TranscodePolicy::BestEffort,
            },
        )
        .expect("request conversion")
}

fn run_response(src: ProtocolId, dst: ProtocolId, body: &[u8]) -> Transcoded<Vec<u8>> {
    direction(src, dst)
        .expect("direction")
        .response(
            body,
            &Plan {
                src,
                dst,
                policy: TranscodePolicy::BestEffort,
            },
        )
        .expect("response conversion")
}

#[test]
fn golden_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let Ok(directions) = fs::read_dir(&root) else {
        return;
    };

    for direction_entry in directions.flatten() {
        if !direction_entry.path().is_dir() {
            continue;
        }
        let direction_name = direction_entry.file_name().to_string_lossy().to_string();
        let (src, dst) = protocols(&direction_name);

        for case_entry in fs::read_dir(direction_entry.path()).unwrap().flatten() {
            let case_path = case_entry.path();
            if !case_path.is_dir() {
                continue;
            }
            let expect = read_json(&case_path.join("expect.json"));
            let request_warnings = expect_list(&expect, "request_warnings");
            let response_warnings = expect_list(&expect, "response_warnings");
            let label = format!(
                "{direction_name}/{}",
                case_entry.file_name().to_string_lossy()
            );

            if let Some(source) = read_optional(&case_path.join("request.src.json")) {
                let expected = read_json(&case_path.join("request.dst.json"));
                let output = run_request(src, dst, &source);
                let actual: Value = serde_json::from_slice(&output.body).unwrap();
                assert_eq!(actual, expected, "{label}: request mismatch");
                assert_warnings(
                    &output.report,
                    &request_warnings,
                    &format!("{label} request"),
                );
            }

            if let Some(source) = read_optional(&case_path.join("response.dst.json")) {
                let expected = read_json(&case_path.join("response.src.json"));
                let output = run_response(src, dst, &source);
                let actual: Value = serde_json::from_slice(&output.body).unwrap();
                assert_eq!(actual, expected, "{label}: response mismatch");
                assert_warnings(
                    &output.report,
                    &response_warnings,
                    &format!("{label} response"),
                );
            }
        }
    }
}
