use super::*;

const REQUEST: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/responses_request.json"
));
const RESPONSE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/responses_response.json"
));
const STREAM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/responses_stream.txt"
));

#[test]
fn request_parses_input_items_and_preserves_unknown_fields() {
    let request: ResponsesRequest = serde_json::from_str(REQUEST).unwrap();
    assert_eq!(request.model, "gpt-5.1");
    assert_eq!(
        request.instructions.as_deref(),
        Some("You are a helpful assistant.")
    );
    let ResponsesInput::Items(items) = request.input.as_ref().unwrap() else {
        panic!("expected input items");
    };
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].type_.as_deref(), Some("message"));
    assert_eq!(items[1].type_.as_deref(), Some("function_call"));
    assert_eq!(items[2].type_.as_deref(), Some("function_call_output"));
    assert_eq!(
        items[1].arguments,
        Some(serde_json::json!("{\"city\":\"Beijing\"}"))
    );
    assert_eq!(request.store, Some(false));
    assert_eq!(
        request.extra.get("vendor_flag"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(request.tools[0].type_, "function");
    assert_eq!(
        request.reasoning.as_ref().unwrap().effort.as_deref(),
        Some("medium")
    );
}

#[test]
fn response_parses_output_items_and_usage() {
    let response: ResponsesResponse = serde_json::from_str(RESPONSE).unwrap();
    assert_eq!(response.id, "resp_123");
    assert_eq!(response.status, "completed");
    assert_eq!(response.output.len(), 3);
    assert_eq!(response.output[0].type_, "reasoning");
    assert_eq!(response.output[1].type_, "message");
    assert_eq!(response.output[2].type_, "function_call");
    assert_eq!(
        response.output[2].arguments,
        Some(serde_json::json!("{\"city\":\"Beijing\"}"))
    );
    let usage = response.usage.as_ref().unwrap();
    assert_eq!(usage.input_tokens, Some(100));
    assert_eq!(
        usage
            .input_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens),
        Some(40)
    );
    assert!(response.extra.contains_key("vendor_meta"));
}

#[test]
fn stream_events_parse() {
    let events: Vec<ResponsesEvent> = STREAM
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 7);
    assert_eq!(events[0].type_, "response.created");
    assert_eq!(events[3].type_, "response.output_text.delta");
    assert_eq!(events[3].delta.as_deref(), Some("Hello"));
    assert_eq!(events[6].type_, "response.completed");
    assert_eq!(events[6].sequence_number, Some(6));
    assert_eq!(
        events[6]
            .response
            .as_ref()
            .and_then(|response| response.usage.as_ref())
            .and_then(|usage| usage.total_tokens),
        Some(15)
    );
}

#[test]
fn error_envelope_parses() {
    let error: ResponsesErrorEnvelope = serde_json::from_str(
        r#"{"error":{"code":"invalid_request","message":"bad","param":"input","type":"invalid_request_error"}}"#,
    )
    .unwrap();
    assert_eq!(error.error.message, "bad");
    assert_eq!(error.error.code.as_deref(), Some("invalid_request"));
}
