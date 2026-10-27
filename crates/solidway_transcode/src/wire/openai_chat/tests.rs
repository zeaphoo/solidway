use super::*;

const REQUEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/chat_request.json"
));
const RESPONSE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/chat_response.json"
));
const STREAM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/chat_stream.txt"
));

#[test]
fn request_parses_unions_and_preserves_unknown_fields() {
    let request: ChatRequest = serde_json::from_str(REQUEST).unwrap();
    assert_eq!(request.model, "gpt-5.1");
    assert_eq!(request.messages.len(), 2);
    assert!(matches!(
        request.messages[0].content,
        Some(ChatContent::Text(_))
    ));
    assert!(matches!(
        request.messages[1].content,
        Some(ChatContent::Parts(_))
    ));
    assert!(matches!(request.stop, Some(ChatStop::Many(_))));
    assert!(matches!(
        request.tool_choice,
        Some(ChatToolChoice::Object(_))
    ));
    assert_eq!(request.max_completion_tokens, Some(1024));
    assert_eq!(
        request.extra.get("vendor_flag"),
        Some(&serde_json::json!(true))
    );

    let roundtrip: serde_json::Value = serde_json::to_value(&request).unwrap();
    assert_eq!(roundtrip["vendor_flag"], serde_json::json!(true));
    assert_eq!(roundtrip["stop"], serde_json::json!(["END", "STOP"]));
}

#[test]
fn response_parses_tool_calls_and_usage_details() {
    let response: ChatResponse = serde_json::from_str(RESPONSE).unwrap();
    assert_eq!(response.id, "chatcmpl-123");
    assert_eq!(response.choices[0].finish_reason, "tool_calls");
    assert_eq!(response.choices[0].message.tool_calls.len(), 1);
    assert_eq!(
        response.choices[0].message.tool_calls[0].function.arguments,
        "{\"city\":\"Beijing\"}"
    );
    let usage = response.usage.as_ref().unwrap();
    assert_eq!(usage.prompt_tokens, Some(100));
    assert_eq!(
        usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens),
        Some(40)
    );
    assert_eq!(
        usage
            .completion_tokens_details
            .as_ref()
            .and_then(|details| details.reasoning_tokens),
        Some(10)
    );
    assert!(response.extra.contains_key("vendor_meta"));
}

#[test]
fn stream_chunks_parse() {
    let chunks: Vec<ChatChunk> = STREAM
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(chunks.len(), 5);
    assert_eq!(
        chunks[0].choices[0].delta.role.as_deref(),
        Some("assistant")
    );
    assert_eq!(
        chunks[1].choices[0].delta.reasoning_content.as_deref(),
        Some("thinking...")
    );
    assert_eq!(chunks[2].choices[0].delta.tool_calls[0].index, Some(0));
    assert_eq!(
        chunks[3].choices[0].delta.tool_calls[0].function.arguments,
        "\"Beijing\"}"
    );
    assert_eq!(
        chunks[4].choices[0].finish_reason.as_deref(),
        Some("tool_calls")
    );
    assert_eq!(chunks[4].usage.as_ref().unwrap().total_tokens, Some(120));
}

#[test]
fn error_envelope_parses() {
    let error: ChatError = serde_json::from_str(
        r#"{"error":{"message":"bad","type":"invalid_request_error","param":"messages","code":"invalid"}}"#,
    )
    .unwrap();
    assert_eq!(error.error.message, "bad");
    assert_eq!(error.error.type_.as_deref(), Some("invalid_request_error"));
}
