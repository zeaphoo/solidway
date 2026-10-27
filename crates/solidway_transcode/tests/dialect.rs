use serde_json::Value;
use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, Transcoded, direction, stream_translator};
use solidway_transcode::report::{Report, TranscodePolicy};

const CHAT_REQUEST: &str = include_str!("fixtures/chat_request.json");
const CHAT_STREAM: &str = include_str!("fixtures/chat_stream.txt");
const RESPONSES_REQUEST: &str = include_str!("fixtures/responses_request.json");
const RESPONSES_RESPONSE: &str = include_str!("fixtures/responses_response.json");
const RESPONSES_STREAM: &str = include_str!("fixtures/responses_stream.txt");
const ANTHROPIC_STREAM: &str = include_str!("fixtures/anthropic_stream.txt");

fn plan(src: ProtocolId, dst: ProtocolId) -> Plan {
    Plan {
        src,
        dst,
        policy: TranscodePolicy::BestEffort,
    }
}

fn request(src: ProtocolId, dst: ProtocolId, body: &str) -> (Value, Report) {
    let direction = direction(src, dst).expect("direction");
    let Transcoded { body, report } = direction.request(body.as_bytes(), &plan(src, dst)).unwrap();
    (serde_json::from_slice(&body).unwrap(), report)
}

fn response(src: ProtocolId, dst: ProtocolId, body: &str) -> (Value, Report) {
    let direction = direction(src, dst).expect("direction");
    let Transcoded { body, report } = direction
        .response(body.as_bytes(), &plan(src, dst))
        .unwrap();
    (serde_json::from_slice(&body).unwrap(), report)
}

fn sse(jsonl: &str) -> String {
    jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("data: {line}\n\n"))
        .collect()
}

fn stream(src: ProtocolId, dst: ProtocolId, jsonl: &str) -> String {
    let mut translator = stream_translator(src, dst, &plan(src, dst)).unwrap();
    let mut out = translator.feed(sse(jsonl).as_bytes()).unwrap();
    out.extend(translator.finish().unwrap());
    String::from_utf8(out).unwrap()
}

fn has_warning(report: &Report, pointer: &str) -> bool {
    report
        .warnings
        .iter()
        .any(|warning| warning.pointer == pointer)
}

#[test]
fn codex_request_sets_store_and_drops_metadata() {
    let body = serde_json::json!({
        "model": "gpt-5.1",
        "messages": [{"role": "user", "content": "hi"}],
        "metadata": {"trace": "abc"},
        "service_tier": "default"
    })
    .to_string();
    let (out, report) = request(ProtocolId::OPENAI_CHAT, ProtocolId::PRIVATE_CODEX, &body);
    assert_eq!(out["store"], false);
    assert_eq!(out["stream"], true);
    assert_eq!(
        out["include"],
        serde_json::json!(["reasoning.encrypted_content"])
    );
    assert_eq!(out["parallel_tool_calls"], true);
    assert!(out.get("metadata").is_none());
    assert!(out.get("service_tier").is_none());
    assert!(has_warning(&report, "/metadata"));
    assert!(has_warning(&report, "/service_tier"));
}

#[test]
fn codex_response_is_decoded_to_chat() {
    let (out, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::PRIVATE_CODEX,
        RESPONSES_RESPONSE,
    );
    assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        out["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
}

#[test]
fn claude_code_request_defaults_max_tokens_and_drops_unsupported_fields() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "messages": [{"role": "user", "content": "hi"}],
        "context_management": {"x": 1},
        "mcp_servers": [{"url": "https://example.com/mcp"}]
    })
    .to_string();
    let (out, report) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::PRIVATE_CLAUDE_CODE,
        &body,
    );
    assert_eq!(out["max_tokens"], 4096);
    assert!(out.get("context_management").is_none());
    assert!(out.get("mcp_servers").is_none());
    assert!(has_warning(&report, "/context_management"));
    assert!(has_warning(&report, "/mcp_servers"));
}

#[test]
fn claude_code_response_drops_container() {
    let body = serde_json::json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-sonnet-4-6",
        "content": [{"type": "text", "text": "hi"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 1, "output_tokens": 1},
        "container": {"id": "c"}
    })
    .to_string();
    let (out, report) = response(
        ProtocolId::PRIVATE_CLAUDE_CODE,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert!(out.get("container").is_none());
    assert!(has_warning(&report, "/container"));
}

#[test]
fn chat_to_claude_code_composes_chat_anthropic_and_dialect_patch() {
    let (out, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::PRIVATE_CLAUDE_CODE,
        CHAT_REQUEST,
    );
    assert_eq!(out["model"], "gpt-5.1");
    assert!(out["max_tokens"].is_number());
    assert!(out.get("context_management").is_none());
    assert!(out["messages"].is_array());
}

#[test]
fn codex_to_claude_code_crosses_bases() {
    let (out, _) = request(
        ProtocolId::PRIVATE_CODEX,
        ProtocolId::PRIVATE_CLAUDE_CODE,
        RESPONSES_REQUEST,
    );
    assert_eq!(out["model"], "gpt-5.1");
    assert_eq!(out["max_tokens"], 1024);
    assert!(out.get("context_management").is_none());
    assert!(out["messages"].is_array());
}

#[test]
fn codex_stream_is_decoded_to_chat() {
    let out = stream(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::PRIVATE_CODEX,
        RESPONSES_STREAM,
    );
    assert!(out.contains("\"content\":\"Hello\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn claude_code_stream_is_decoded_from_chat_to_anthropic() {
    let out = stream(
        ProtocolId::PRIVATE_CLAUDE_CODE,
        ProtocolId::OPENAI_CHAT,
        CHAT_STREAM,
    );
    assert!(out.contains("event: message_start"));
    assert!(out.contains("event: message_stop"));
}

#[test]
fn codex_stream_is_encoded_from_chat_to_responses() {
    let out = stream(
        ProtocolId::PRIVATE_CODEX,
        ProtocolId::OPENAI_CHAT,
        CHAT_STREAM,
    );
    assert!(out.contains("event: response.created"));
    assert!(out.contains("event: response.completed"));
}

#[test]
fn codex_stream_round_trips_with_responses() {
    let out = stream(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::PRIVATE_CODEX,
        RESPONSES_STREAM,
    );
    assert!(out.contains("\"type\":\"response.created\""));
    assert!(out.contains("\"type\":\"response.completed\""));

    let out = stream(
        ProtocolId::PRIVATE_CODEX,
        ProtocolId::OPENAI_RESPONSES,
        RESPONSES_STREAM,
    );
    assert!(out.contains("\"type\":\"response.created\""));
    assert!(out.contains("\"type\":\"response.completed\""));
}

#[test]
fn claude_code_stream_round_trips_with_anthropic() {
    let out = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::PRIVATE_CLAUDE_CODE,
        ANTHROPIC_STREAM,
    );
    assert!(out.contains("\"type\":\"message_start\""));
    assert!(out.contains("\"type\":\"message_stop\""));

    let out = stream(
        ProtocolId::PRIVATE_CLAUDE_CODE,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_STREAM,
    );
    assert!(out.contains("\"type\":\"message_start\""));
    assert!(out.contains("\"type\":\"message_stop\""));
}

#[test]
fn cross_base_dialect_streams() {
    let out = stream(
        ProtocolId::PRIVATE_CODEX,
        ProtocolId::PRIVATE_CLAUDE_CODE,
        ANTHROPIC_STREAM,
    );
    assert!(out.contains("event: response.created"));
    assert!(out.contains("event: response.completed"));

    let out = stream(
        ProtocolId::PRIVATE_CLAUDE_CODE,
        ProtocolId::PRIVATE_CODEX,
        RESPONSES_STREAM,
    );
    assert!(out.contains("event: message_start"));
    assert!(out.contains("event: message_stop"));
}

#[test]
fn composed_caps_are_narrowed() {
    use solidway_transcode::protocol::Support;

    let codex = direction(ProtocolId::OPENAI_CHAT, ProtocolId::PRIVATE_CODEX)
        .unwrap()
        .caps();
    assert!(matches!(
        codex.request.custom_tools,
        Support::Unsupported(_)
    ));
    assert!(matches!(
        codex.request.hosted_tools,
        Support::Unsupported(_)
    ));

    let claude = direction(ProtocolId::OPENAI_CHAT, ProtocolId::PRIVATE_CLAUDE_CODE)
        .unwrap()
        .caps();
    assert!(matches!(
        claude.request.custom_tools,
        Support::Unsupported(_)
    ));
}

#[test]
fn claude_code_defaults_metadata_user_id() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "hi"}]
    })
    .to_string();
    let (out, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::PRIVATE_CLAUDE_CODE,
        &body,
    );
    assert_eq!(out["metadata"]["user_id"], "claude-code");
}
