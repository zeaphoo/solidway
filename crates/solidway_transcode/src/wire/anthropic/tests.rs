use super::*;

const REQUEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/anthropic_request.json"
));
const RESPONSE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/anthropic_response.json"
));
const STREAM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/anthropic_stream.txt"
));

#[test]
fn request_parses_system_messages_and_content_blocks() {
    let request: AnthropicRequest = serde_json::from_str(REQUEST).unwrap();
    assert_eq!(request.model, "claude-sonnet-4-6");
    assert_eq!(request.max_tokens, Some(1024));
    assert!(matches!(request.system, Some(AnthropicSystem::Blocks(_))));
    assert_eq!(request.messages.len(), 3);
    assert!(matches!(
        request.messages[0].content,
        AnthropicMessageContent::Blocks(_)
    ));
    assert_eq!(request.tools.len(), 1);
    assert_eq!(
        request
            .thinking
            .as_ref()
            .and_then(|thinking| thinking.budget_tokens),
        Some(2048)
    );
    assert_eq!(request.stop_sequences, vec!["END"]);
    assert_eq!(request.top_k, Some(40));
    assert_eq!(
        request.extra.get("vendor_flag"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn response_parses_thinking_and_tool_use() {
    let response: AnthropicResponse = serde_json::from_str(RESPONSE).unwrap();
    assert_eq!(response.id, "msg_123");
    assert_eq!(response.stop_reason.as_deref(), Some("tool_use"));
    assert_eq!(response.content.len(), 3);
    assert_eq!(response.content[0].type_, "thinking");
    assert_eq!(response.content[0].signature.as_deref(), Some("sig_123"));
    assert_eq!(response.content[2].type_, "tool_use");
    let usage = response.usage.as_ref().unwrap();
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(usage.cache_read_input_tokens, Some(40));
    assert_eq!(usage.cache_creation_input_tokens, Some(10));
}

#[test]
fn stream_events_parse() {
    let events: Vec<AnthropicEvent> = STREAM
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 10);
    assert_eq!(events[0].type_, "message_start");
    assert_eq!(events[1].index, Some(0));
    assert_eq!(
        events[2]
            .delta
            .as_ref()
            .and_then(|delta| delta.text.as_deref()),
        Some("Hello")
    );
    assert_eq!(
        events[6]
            .delta
            .as_ref()
            .and_then(|delta| delta.partial_json.as_deref()),
        Some("\"Beijing\"}")
    );
    assert_eq!(
        events[8]
            .delta
            .as_ref()
            .and_then(|delta| delta.stop_reason.as_deref()),
        Some("tool_use")
    );
}

#[test]
fn error_envelope_parses() {
    let error: AnthropicError = serde_json::from_str(
        r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad"},"request_id":"req_1"}"#,
    )
    .unwrap();
    assert_eq!(error.error.message, "bad");
    assert_eq!(error.request_id.as_deref(), Some("req_1"));
}
