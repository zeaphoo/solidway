use serde_json::Value;
use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, Transcoded, direction, stream_translator};
use solidway_transcode::report::{Report, TranscodePolicy};

const CHAT_REQUEST: &str = include_str!("fixtures/chat_request.json");
const CHAT_RESPONSE: &str = include_str!("fixtures/chat_response.json");
const CHAT_STREAM: &str = include_str!("fixtures/chat_stream.txt");
const RESPONSES_REQUEST: &str = include_str!("fixtures/responses_request.json");
const RESPONSES_RESPONSE: &str = include_str!("fixtures/responses_response.json");
const RESPONSES_STREAM: &str = include_str!("fixtures/responses_stream.txt");
const ANTHROPIC_REQUEST: &str = include_str!("fixtures/anthropic_request.json");
const ANTHROPIC_RESPONSE: &str = include_str!("fixtures/anthropic_response.json");
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

fn strict_plan(src: ProtocolId, dst: ProtocolId) -> Plan {
    Plan {
        src,
        dst,
        policy: TranscodePolicy::Strict,
    }
}

fn stream_with_report(src: ProtocolId, dst: ProtocolId, jsonl: &str) -> (String, Report) {
    let input: String = jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("data: {line}\n\n"))
        .collect();
    let mut translator = stream_translator(src, dst, &plan(src, dst)).unwrap();
    let mut out = translator.feed(input.as_bytes()).unwrap();
    out.extend(translator.finish().unwrap());
    let report = translator.report().clone();
    (String::from_utf8(out).unwrap(), report)
}

fn stream(src: ProtocolId, dst: ProtocolId, jsonl: &str) -> String {
    stream_with_report(src, dst, jsonl).0
}

fn has_warning(report: &Report, pointer: &str) -> bool {
    report
        .warnings
        .iter()
        .any(|warning| warning.pointer == pointer)
}

#[test]
fn chat_to_anthropic_request_and_response() {
    let (request, report) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        CHAT_REQUEST,
    );
    assert_eq!(request["model"], "gpt-5.1");
    assert_eq!(request["max_tokens"], 1024);
    assert_eq!(request["tools"][0]["name"], "get_weather");
    assert_eq!(request["tool_choice"]["type"], "tool");
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|block| block["type"] == "image"))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.pointer.contains("stream_options"))
    );

    let (response, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_RESPONSE,
    );
    assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        response["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
    assert!(
        response["choices"][0]["message"]["reasoning_content"]
            .as_str()
            .unwrap()
            .contains("weather tool")
    );
    assert_eq!(response["usage"]["prompt_tokens"], 150);
    assert_eq!(response["usage"]["completion_tokens"], 20);
}

#[test]
fn chat_to_anthropic_stream() {
    let out = stream(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_STREAM,
    );
    assert!(out.contains("data: [DONE]"));
    assert!(out.contains("\"content\":\"Hello\""));
    assert!(out.contains("\"tool_calls\""));
    assert!(out.contains("\"finish_reason\":\"tool_calls\""));
}

#[test]
fn anthropic_to_chat_request_and_response() {
    let (request, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        ANTHROPIC_REQUEST,
    );
    assert_eq!(request["model"], "claude-sonnet-4-6");
    assert_eq!(request["messages"][0]["role"], "system");
    assert_eq!(request["max_completion_tokens"], 1024);
    assert_eq!(request["stop"], serde_json::json!(["END"]));
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool")
    );
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["tool_calls"]
                .as_array()
                .is_some_and(|calls| !calls.is_empty()))
    );

    let (response, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        CHAT_RESPONSE,
    );
    assert_eq!(response["content"][0]["type"], "thinking");
    assert_eq!(response["content"][0]["signature"], "");
    assert_eq!(response["stop_reason"], "tool_use");
    assert_eq!(response["usage"]["input_tokens"], 60);
    assert_eq!(response["usage"]["cache_read_input_tokens"], 40);
}

#[test]
fn anthropic_to_chat_stream() {
    let out = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        CHAT_STREAM,
    );
    assert!(out.contains("event: message_start"));
    assert!(out.contains("\"type\":\"thinking_delta\""));
    assert!(out.contains("\"type\":\"input_json_delta\""));
    assert!(out.contains("event: message_stop"));
}

#[test]
fn chat_to_responses_request_and_response() {
    let (request, report) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        CHAT_REQUEST,
    );
    assert_eq!(request["model"], "gpt-5.1");
    assert!(request.get("instructions").is_none());
    assert_eq!(request["max_output_tokens"], 1024);
    assert!(
        request["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["role"] == "system")
    );
    assert_eq!(request["tools"][0]["type"], "function");
    assert_eq!(request["reasoning"]["effort"], "medium");
    assert!(
        request["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "message")
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.pointer.contains("stop"))
    );

    let (response, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        RESPONSES_RESPONSE,
    );
    assert_eq!(response["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        response["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
    assert_eq!(
        response["choices"][0]["message"]["reasoning_content"],
        "Thinking..."
    );
    assert_eq!(response["usage"]["prompt_tokens"], 100);
    assert_eq!(
        response["usage"]["completion_tokens_details"]["reasoning_tokens"],
        10
    );
}

#[test]
fn chat_to_responses_stream() {
    let out = stream(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        RESPONSES_STREAM,
    );
    assert!(out.contains("\"content\":\"Hello\""));
    assert!(out.contains("\"finish_reason\":\"stop\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn responses_to_chat_request_and_response() {
    let (request, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        RESPONSES_REQUEST,
    );
    assert_eq!(request["model"], "gpt-5.1");
    assert_eq!(request["messages"][0]["role"], "system");
    assert_eq!(request["max_completion_tokens"], 1024);
    assert_eq!(request["reasoning_effort"], "medium");
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "tool")
    );
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["tool_calls"]
                .as_array()
                .is_some_and(|calls| !calls.is_empty()))
    );

    let (response, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        CHAT_RESPONSE,
    );
    assert_eq!(response["status"], "completed");
    assert!(
        response["output"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "reasoning")
    );
    assert!(
        response["output"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call")
    );
    assert_eq!(response["usage"]["input_tokens"], 100);
}

#[test]
fn responses_to_chat_stream() {
    let out = stream(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        CHAT_STREAM,
    );
    assert!(out.contains("event: response.created"));
    assert!(out.contains("event: response.reasoning_summary_text.delta"));
    assert!(out.contains("event: response.completed"));
}

#[test]
fn anthropic_to_responses_request_and_response() {
    let (request, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        ANTHROPIC_REQUEST,
    );
    assert_eq!(request["model"], "claude-sonnet-4-6");
    assert!(request.get("instructions").is_none());
    assert_eq!(request["input"][0]["role"], "developer");
    assert_eq!(
        request["input"][0]["content"][0]["text"],
        "You are a helpful assistant."
    );
    assert_eq!(request["max_output_tokens"], 1024);
    assert_eq!(request["reasoning"]["effort"], "medium");
    assert_eq!(
        request["include"],
        serde_json::json!(["reasoning.encrypted_content"])
    );
    assert!(
        request["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call")
    );
    assert!(
        request["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call_output")
    );

    let (response, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        RESPONSES_RESPONSE,
    );
    assert_eq!(response["content"][0]["type"], "thinking");
    assert_eq!(response["content"][0]["thinking"], "Thinking...");
    assert_eq!(response["stop_reason"], "tool_use");
    assert_eq!(response["usage"]["input_tokens"], 60);
    assert_eq!(response["usage"]["cache_read_input_tokens"], 40);
}

#[test]
fn anthropic_to_responses_stream() {
    let out = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        RESPONSES_STREAM,
    );
    assert!(out.contains("event: message_start"));
    assert!(out.contains("\"type\":\"text_delta\""));
    assert!(out.contains("event: message_stop"));
}

#[test]
fn responses_to_anthropic_request_and_response() {
    let (request, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        RESPONSES_REQUEST,
    );
    assert_eq!(request["model"], "gpt-5.1");
    assert_eq!(request["system"], "You are a helpful assistant.");
    assert_eq!(request["max_tokens"], 1024);
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|block| block["type"] == "tool_use"))
    );
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|block| block["type"] == "tool_result"))
    );

    let (response, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_RESPONSE,
    );
    assert_eq!(response["output"][0]["type"], "reasoning");
    assert_eq!(response["output"][0]["encrypted_content"], "sig_123");
    assert_eq!(response["status"], "completed");
    assert_eq!(response["usage"]["input_tokens"], 150);
}

#[test]
fn responses_to_anthropic_stream() {
    let out = stream(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_STREAM,
    );
    assert!(out.contains("event: response.created"));
    assert!(out.contains("event: response.output_text.delta"));
    assert!(out.contains("event: response.completed"));
}

#[test]
fn stream_output_is_deterministic() {
    let first = stream(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_STREAM,
    );
    let second = stream(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_STREAM,
    );
    assert_eq!(first, second);
}

#[test]
fn strict_policy_rejects_unsupported_fields() {
    let plan = Plan {
        src: ProtocolId::OPENAI_CHAT,
        dst: ProtocolId::ANTHROPIC_MESSAGES,
        policy: TranscodePolicy::Strict,
    };
    let direction = direction(ProtocolId::OPENAI_CHAT, ProtocolId::ANTHROPIC_MESSAGES).unwrap();
    let error = direction
        .request(CHAT_REQUEST.as_bytes(), &plan)
        .unwrap_err();
    assert!(matches!(
        error,
        solidway_transcode::report::Error::Unsupported(_)
    ));
}

#[test]
fn chat_to_anthropic_warns_on_unmapped_request_fields() {
    let body = serde_json::json!({
        "model": "gpt-5.1",
        "instructions": "be concise",
        "messages": [
            {
                "role": "assistant",
                "name": "bob",
                "content": "hi",
                "function_call": {"name": "get_weather", "arguments": "{\"city\":\"Beijing\"}"}
            }
        ]
    })
    .to_string();
    let (out, report) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert!(has_warning(&report, "/messages/0/name"));
    assert!(has_warning(&report, "/messages/0/function_call"));
    assert_eq!(out["system"][0]["text"], "be concise");
    assert!(out.to_string().contains("hi"));
    assert!(!out.to_string().contains("\"type\":\"tool_use\""));
}

#[test]
fn chat_to_anthropic_warns_on_unmapped_response_fields() {
    let body = serde_json::json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "m",
        "content": [{
            "type": "text",
            "text": "hi",
            "cache_control": {"type": "ephemeral"},
            "citations": [{"type": "web_search_result_location"}]
        }],
        "stop_reason": "end_turn",
        "stop_sequence": "END",
        "usage": {"input_tokens": 1, "output_tokens": 1},
        "container": {"id": "c"}
    })
    .to_string();
    let (_, report) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert!(has_warning(&report, "/stop_sequence"));
    assert!(has_warning(&report, "/container"));
    assert!(has_warning(&report, "/content/0/cache_control"));
    assert!(has_warning(&report, "/content/0/citations"));
}

#[test]
fn anthropic_to_chat_warns_and_strict_rejects_mcp_servers() {
    let body = serde_json::json!({
        "model": "m",
        "max_tokens": 10,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "text",
                "text": "hi",
                "cache_control": {"type": "ephemeral"}
            }]
        }],
        "mcp_servers": [{"url": "https://example.com/mcp"}]
    })
    .to_string();
    let (_, report) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert!(has_warning(&report, "/mcp_servers"));
    assert!(has_warning(&report, "/messages/0/content/0/cache_control"));

    let direction = direction(ProtocolId::ANTHROPIC_MESSAGES, ProtocolId::OPENAI_CHAT).unwrap();
    let error = direction
        .request(
            body.as_bytes(),
            &strict_plan(ProtocolId::ANTHROPIC_MESSAGES, ProtocolId::OPENAI_CHAT),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        solidway_transcode::report::Error::Unsupported(_)
    ));
}

#[test]
fn chat_to_responses_warns_on_unmapped_request_fields() {
    let body = serde_json::json!({
        "model": "m",
        "instructions": "be concise",
        "messages": [
            {"role": "system", "content": "be safe"},
            {"role": "user", "content": "hi", "name": "alice"}
        ],
        "function_call": "auto"
    })
    .to_string();
    let (out, report) = request(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, &body);
    assert!(has_warning(&report, "/messages/1/name"));
    assert_eq!(out["tool_choice"], "auto");
    assert_eq!(out["instructions"], "be concise");
    assert!(
        out["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["role"] == "system")
    );
}

#[test]
fn responses_to_chat_warns_on_unmapped_request_fields() {
    let body = serde_json::json!({
        "model": "m",
        "text": {"format": {"type": "json_schema", "schema": {}}, "verbosity": "high"},
        "input": [{
            "type": "message",
            "role": "user",
            "id": "msg_1",
            "status": "completed",
            "content": [{
                "type": "input_text",
                "text": "hi",
                "annotations": [{"type": "url_citation"}]
            }]
        }]
    })
    .to_string();
    let (_, report) = request(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, &body);
    assert!(has_warning(&report, "/text/verbosity"));
    assert!(has_warning(&report, "/input/0/id"));
    assert!(has_warning(&report, "/input/0/status"));
    assert!(has_warning(&report, "/input/0/content/0/annotations"));
}

#[test]
fn anthropic_to_responses_warns_on_unmapped_request_fields() {
    let body = serde_json::json!({
        "model": "m",
        "max_tokens": 10,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "text",
                "text": "hi",
                "citations": [{"type": "x"}]
            }]
        }],
        "tools": [{
            "name": "t",
            "input_schema": {},
            "cache_control": {"type": "ephemeral"}
        }],
        "mcp_servers": [{"url": "https://example.com/mcp"}]
    })
    .to_string();
    let (_, report) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert!(has_warning(&report, "/mcp_servers"));
    assert!(has_warning(&report, "/tools/0/cache_control"));
    assert!(has_warning(&report, "/messages/0/content/0/citations"));
}

#[test]
fn responses_to_anthropic_warns_on_unmapped_request_fields() {
    let body = serde_json::json!({
        "model": "m",
        "text": {"format": {"type": "json_schema", "schema": {}}, "verbosity": "high"},
        "reasoning": {"effort": "medium", "summary": "auto"},
        "input": [{
            "type": "message",
            "role": "user",
            "id": "m1",
            "status": "completed",
            "content": "hi"
        }]
    })
    .to_string();
    let (_, report) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert!(has_warning(&report, "/text/verbosity"));
    assert!(has_warning(&report, "/reasoning/summary"));
    assert!(has_warning(&report, "/input/0/id"));
    assert!(has_warning(&report, "/input/0/status"));
}

#[test]
fn anthropic_to_chat_stream_warns_on_extra_choice_and_refusal() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","service_tier":"default","choices":[{"index":0,"delta":{"refusal":"no"},"finish_reason":null},{"index":1,"delta":{"content":"ignored"},"finish_reason":null}]}"#;
    let (out, report) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        jsonl,
    );
    assert!(out.contains("\"text\":\"no\""));
    assert!(has_warning(&report, "/choices"));
    assert!(has_warning(&report, "/delta/refusal"));
    assert!(has_warning(&report, "/service_tier"));
}

#[test]
fn chat_to_anthropic_stream_warns_on_signature_and_ping() {
    let jsonl = r#"{"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"m","content":[],"stop_reason":null,"usage":{"input_tokens":1,"output_tokens":0}}}
{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":"sig"}}
{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig"}}
{"type":"ping"}
{"type":"message_stop"}"#;
    let (_, report) = stream_with_report(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        jsonl,
    );
    assert!(has_warning(&report, "/content_block/signature"));
    assert!(has_warning(&report, "/delta/signature"));
    assert!(has_warning(&report, "/ping"));
}

#[test]
fn responses_to_chat_stream_warns_on_extra_choice_and_refusal() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"refusal":"no"},"finish_reason":null},{"index":1,"delta":{"content":"ignored"},"finish_reason":null}]}"#;
    let (out, report) =
        stream_with_report(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, jsonl);
    assert!(out.contains("\"delta\":\"no\""));
    assert!(has_warning(&report, "/choices"));
    assert!(has_warning(&report, "/delta/refusal"));
}

#[test]
fn chat_to_responses_stream_warns_on_service_tier_and_unknown_item() {
    let jsonl = r#"{"type":"response.created","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"in_progress","output":[],"service_tier":"default"},"sequence_number":0}
{"type":"response.output_item.added","output_index":0,"item":{"type":"unknown_call","id":"ws"},"sequence_number":1}
{"type":"response.completed","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}},"sequence_number":2}"#;
    let (_, report) =
        stream_with_report(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, jsonl);
    assert!(has_warning(&report, "/response/service_tier"));
    assert!(has_warning(&report, "/item/type"));
}

const PARALLEL_CHAT: &str = include_str!("fixtures/parallel_tools_chat_request.json");
const PARALLEL_ANTHROPIC: &str = include_str!("fixtures/parallel_tools_anthropic_request.json");
const PARALLEL_RESPONSES: &str = include_str!("fixtures/parallel_tools_responses_request.json");
const MULTIMODAL_CHAT: &str = include_str!("fixtures/multimodal_chat_request.json");
const MULTIMODAL_ANTHROPIC: &str = include_str!("fixtures/multimodal_anthropic_request.json");
const MULTIMODAL_RESPONSES: &str = include_str!("fixtures/multimodal_responses_request.json");
const ANTHROPIC_TRUNCATED: &str = include_str!("fixtures/anthropic_stream_truncated.txt");
const CHAT_TRUNCATED: &str = include_str!("fixtures/chat_stream_truncated.txt");
const RESPONSES_TRUNCATED: &str = include_str!("fixtures/responses_stream_truncated.txt");

fn count_type(items: &Value, field: &str, value: &str) -> usize {
    items
        .as_array()
        .map(|items| items.iter().filter(|item| item[field] == value).count())
        .unwrap_or(0)
}

#[test]
fn parallel_tools_chat_to_anthropic() {
    let (out, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        PARALLEL_CHAT,
    );
    let assistant = &out["messages"][1]["content"];
    assert_eq!(count_type(assistant, "type", "tool_use"), 2);
    assert_eq!(count_type(assistant, "type", "text"), 2);
    assert_eq!(count_type(assistant, "type", "thinking"), 0);
    assert!(assistant.as_array().unwrap().iter().any(|block| {
        block["text"]
            .as_str()
            .is_some_and(|text| text.contains("<thinking>Need two weather calls.</thinking>"))
    }));
    let results = &out["messages"][2]["content"];
    assert_eq!(count_type(results, "type", "tool_result"), 2);
}

#[test]
fn parallel_tools_chat_to_responses() {
    let (out, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        PARALLEL_CHAT,
    );
    let input = &out["input"];
    assert_eq!(count_type(input, "type", "function_call"), 2);
    assert_eq!(count_type(input, "type", "function_call_output"), 2);
    assert_eq!(count_type(input, "type", "reasoning"), 0);
    assert_eq!(count_type(input, "type", "message"), 2);
    let assistant = input
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "message" && item["role"] == "assistant")
        .unwrap();
    assert!(assistant["content"].as_array().unwrap().iter().any(|part| {
        part["text"]
            .as_str()
            .is_some_and(|text| text.contains("<thinking>Need two weather calls.</thinking>"))
    }));
}

#[test]
fn parallel_tools_anthropic_to_chat() {
    let (out, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        PARALLEL_ANTHROPIC,
    );
    let assistant = out["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    assert_eq!(assistant["tool_calls"].as_array().unwrap().len(), 2);
    assert_eq!(assistant["reasoning_content"], "Need two weather calls.");
    assert_eq!(assistant["content"], "Checking both.");
    let tool_messages = out["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    assert_eq!(tool_messages, 2);
}

#[test]
fn parallel_tools_anthropic_to_responses() {
    let (out, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        PARALLEL_ANTHROPIC,
    );
    let input = &out["input"];
    assert_eq!(count_type(input, "type", "function_call"), 2);
    assert_eq!(count_type(input, "type", "function_call_output"), 2);
    assert_eq!(count_type(input, "type", "reasoning"), 1);
}

#[test]
fn parallel_tools_responses_to_chat() {
    let (out, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        PARALLEL_RESPONSES,
    );
    let assistant = out["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    assert_eq!(assistant["tool_calls"].as_array().unwrap().len(), 2);
    assert_eq!(assistant["reasoning_content"], "Need two weather calls.");
    let tool_messages = out["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .count();
    assert_eq!(tool_messages, 2);
}

#[test]
fn parallel_tools_responses_to_anthropic() {
    let (out, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        PARALLEL_RESPONSES,
    );
    let assistant = out["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    let assistant_content = &assistant["content"];
    assert_eq!(count_type(assistant_content, "type", "tool_use"), 2);
    assert_eq!(count_type(assistant_content, "type", "thinking"), 1);
    assert_eq!(count_type(assistant_content, "type", "text"), 1);
    let user = out["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| {
            message["role"] == "user"
                && message["content"]
                    .as_array()
                    .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_result"))
        })
        .unwrap();
    assert_eq!(count_type(&user["content"], "type", "tool_result"), 2);
}

#[test]
fn multimodal_chat_to_anthropic_and_responses() {
    let (anthropic, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        MULTIMODAL_CHAT,
    );
    let blocks = &anthropic["messages"][0]["content"];
    assert_eq!(count_type(blocks, "type", "image"), 1);
    assert_eq!(count_type(blocks, "type", "document"), 1);
    assert_eq!(blocks[1]["source"]["type"], "base64");
    assert_eq!(blocks[2]["source"]["type"], "base64");

    let (responses, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        MULTIMODAL_CHAT,
    );
    let parts = &responses["input"][0]["content"];
    assert_eq!(count_type(parts, "type", "input_image"), 1);
    assert_eq!(count_type(parts, "type", "input_file"), 1);
    assert!(
        parts[1]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png")
    );
    assert!(
        parts[2]["file_data"]
            .as_str()
            .unwrap()
            .starts_with("data:application/pdf")
    );
}

#[test]
fn multimodal_anthropic_to_chat_and_responses() {
    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        MULTIMODAL_ANTHROPIC,
    );
    let parts = &chat["messages"][0]["content"];
    assert_eq!(count_type(parts, "type", "image_url"), 1);
    assert_eq!(count_type(parts, "type", "file"), 1);
    assert!(
        parts[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png")
    );
    assert!(
        parts[2]["file"]["file_data"]
            .as_str()
            .unwrap()
            .starts_with("data:application/pdf")
    );

    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        MULTIMODAL_ANTHROPIC,
    );
    let parts = &responses["input"][0]["content"];
    assert_eq!(count_type(parts, "type", "input_image"), 1);
    assert_eq!(count_type(parts, "type", "input_file"), 1);
}

#[test]
fn multimodal_responses_to_chat_and_anthropic() {
    let (chat, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        MULTIMODAL_RESPONSES,
    );
    let parts = &chat["messages"][0]["content"];
    assert_eq!(count_type(parts, "type", "image_url"), 1);
    assert_eq!(count_type(parts, "type", "file"), 1);

    let (anthropic, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        MULTIMODAL_RESPONSES,
    );
    let blocks = &anthropic["messages"][0]["content"];
    assert_eq!(count_type(blocks, "type", "image"), 1);
    assert_eq!(count_type(blocks, "type", "document"), 1);
}

#[test]
fn error_bodies_are_mapped_for_all_directions() {
    let chat_error =
        r#"{"error":{"message":"bad","type":"invalid_request_error","code":"invalid"}}"#;
    let responses_error =
        r#"{"error":{"message":"bad","type":"invalid_request_error","code":"invalid"}}"#;
    let anthropic_error =
        r#"{"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}"#;

    let (out, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        anthropic_error,
    );
    assert_eq!(out["error"]["message"], "overloaded");
    assert_eq!(out["error"]["type"], "server_error");
    assert_eq!(out["error"]["code"], "overloaded_error");

    let (out, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        chat_error,
    );
    assert_eq!(out["type"], "error");
    assert_eq!(out["error"]["type"], "invalid_request_error");
    assert_eq!(out["error"]["message"], "bad");

    let (out, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        responses_error,
    );
    assert_eq!(out["error"]["message"], "bad");
    assert_eq!(out["error"]["code"], "invalid");

    let (out, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        chat_error,
    );
    assert_eq!(out["error"]["code"], "invalid");
    assert_eq!(out["error"]["type"], "invalid_request_error");

    let (out, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        responses_error,
    );
    assert_eq!(out["type"], "error");
    assert_eq!(out["error"]["type"], "invalid_request_error");

    let (out, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        anthropic_error,
    );
    assert_eq!(out["error"]["type"], "server_error");
    assert_eq!(out["error"]["message"], "overloaded");
}

fn assert_truncated(report: &Report) {
    assert!(
        report.warnings.iter().any(|warning| matches!(
            warning.code,
            solidway_transcode::report::WarningCode::Truncated
        )),
        "expected a Truncated warning, got {:?}",
        report.warnings
    );
}

#[test]
fn truncated_streams_finalize_for_all_targets() {
    let (out, report) = stream_with_report(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_TRUNCATED,
    );
    assert!(out.contains("data: [DONE]"));
    assert!(out.contains("\"finish_reason\":\"stop\""));
    assert_truncated(&report);

    let (out, report) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        CHAT_TRUNCATED,
    );
    assert!(out.contains("event: message_stop"));
    assert_truncated(&report);

    let (out, report) = stream_with_report(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        RESPONSES_TRUNCATED,
    );
    assert!(out.contains("data: [DONE]"));
    assert_truncated(&report);

    let (out, report) = stream_with_report(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        CHAT_TRUNCATED,
    );
    assert!(out.contains("event: response.completed"));
    assert_truncated(&report);

    let (out, report) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        RESPONSES_TRUNCATED,
    );
    assert!(out.contains("event: message_stop"));
    assert_truncated(&report);

    let (out, report) = stream_with_report(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        ANTHROPIC_TRUNCATED,
    );
    assert!(out.contains("event: response.completed"));
    assert_truncated(&report);
}

#[test]
fn temperature_is_stripped_for_reasoning_models() {
    let chat_body = serde_json::json!({
        "model": "gpt-5.1",
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.2
    })
    .to_string();
    let (anthropic, report) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        &chat_body,
    );
    assert!(anthropic.get("temperature").is_none());
    assert!(has_warning(&report, "/temperature"));
    let (responses, report) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &chat_body,
    );
    assert!(responses.get("temperature").is_none());
    assert!(has_warning(&report, "/temperature"));

    let responses_body = serde_json::json!({
        "model": "gpt-5.1",
        "input": "hi",
        "temperature": 0.2
    })
    .to_string();
    let (chat, report) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &responses_body,
    );
    assert!(chat.get("temperature").is_none());
    assert!(has_warning(&report, "/temperature"));

    let anthropic_body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "hi"}],
        "temperature": 0.2
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &anthropic_body,
    );
    assert_eq!(chat["temperature"], 0.2);
}

#[test]
fn responses_output_tokens_are_floored() {
    let chat_body = serde_json::json!({
        "model": "gpt-5.1",
        "messages": [{"role": "user", "content": "hi"}],
        "max_completion_tokens": 10
    })
    .to_string();
    let (responses, report) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &chat_body,
    );
    assert_eq!(responses["max_output_tokens"], 128);
    assert!(has_warning(&report, "/max_output_tokens"));

    let anthropic_body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "hi"}]
    })
    .to_string();
    let (responses, report) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &anthropic_body,
    );
    assert_eq!(responses["max_output_tokens"], 128);
    assert!(has_warning(&report, "/max_output_tokens"));
}

#[test]
fn tool_result_images_are_lifted_to_user_messages() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "read image"},
            {"role": "assistant", "content": [
                {"type": "tool_use", "id": "toolu_1", "name": "Read", "input": {"path": "/tmp/a.png"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1", "content": [
                    {"type": "text", "text": "image follows"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBOR"}}
                ]}
            ]}
        ]
    })
    .to_string();

    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    let messages = chat["messages"].as_array().unwrap();
    assert!(messages.iter().any(|message| message["role"] == "tool"));
    let image_message = messages
        .iter()
        .find(|message| {
            message["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "image_url"))
        })
        .unwrap();
    assert!(
        image_message["content"][0]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );

    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    let input = responses["input"].as_array().unwrap();
    assert!(
        input
            .iter()
            .any(|item| item["type"] == "function_call_output")
    );
    let image_item = input
        .iter()
        .find(|item| {
            item["content"]
                .as_array()
                .is_some_and(|parts| parts.iter().any(|part| part["type"] == "input_image"))
        })
        .unwrap();
    assert!(
        image_item["content"][0]["image_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
}

#[test]
fn undeclared_tool_choice_is_dropped() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"name": "get_weather", "input_schema": {"type": "object"}}],
        "tool_choice": {"type": "tool", "name": "missing"}
    })
    .to_string();

    let (chat, report) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert!(chat.get("tool_choice").is_none());
    assert!(has_warning(&report, "/tool_choice"));

    let (responses, report) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert!(responses.get("tool_choice").is_none());
    assert!(has_warning(&report, "/tool_choice"));
}

#[test]
fn disabled_thinking_overrides_output_effort() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "hi"}],
        "thinking": {"type": "disabled"},
        "output_config": {"effort": "high"}
    })
    .to_string();

    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert_eq!(chat["reasoning_effort"], "none");

    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert!(responses.get("reasoning").is_none());
}

#[test]
fn reasoning_only_chat_response_falls_back_to_text() {
    let body = serde_json::json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "m",
        "content": [{"type": "thinking", "thinking": "only thought", "signature": "sig"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 1, "output_tokens": 1}
    })
    .to_string();
    let (chat, report) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert_eq!(chat["choices"][0]["message"]["content"], "only thought");
    assert!(has_warning(&report, "/content"));
}

#[test]
fn responses_developer_messages_become_anthropic_system() {
    let body = serde_json::json!({
        "model": "m",
        "instructions": "base",
        "input": [
            {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "extra"}]},
            {"type": "message", "role": "user", "content": "hi"}
        ]
    })
    .to_string();
    let (out, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert_eq!(out["system"], "base\nextra");
    assert_eq!(out["messages"].as_array().unwrap().len(), 1);
    assert_eq!(out["messages"][0]["role"], "user");
}

#[test]
fn chat_to_anthropic_stream_handles_late_tool_name() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_late"}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"late_tool"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        jsonl,
    );
    assert!(out.contains("\"name\":\"late_tool\""));
    assert!(out.contains("\"partial_json\":\"{}\""));
}

#[test]
fn chat_to_anthropic_stream_buffers_args_before_name() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_early","function":{"arguments":"{\"city\":"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"SF\""}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"get_weather","arguments":"}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        jsonl,
    );
    let start = out.find("\"name\":\"get_weather\"").unwrap();
    let first_delta = out.find("\"partial_json\"").unwrap();
    assert!(
        start < first_delta,
        "delta emitted before content_block_start"
    );
    assert!(out.contains("{\\\"city\\\":"));
    assert!(out.contains("\\\"SF\\\""));
}

#[test]
fn chat_to_anthropic_stream_emits_empty_tool_args_placeholder() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_empty","type":"function","function":{"name":"noop","arguments":""}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        jsonl,
    );
    assert!(out.contains("\"name\":\"noop\""));
    assert!(out.contains("\"partial_json\":\"{}\""));
}

#[test]
fn responses_to_chat_lowers_custom_namespace_and_tool_search() {
    let body = serde_json::json!({
        "model": "m",
        "input": "hi",
        "tools": [
            {"type": "custom", "name": "exec", "description": "run"},
            {"type": "namespace", "name": "fs", "tools": [
                {"type": "function", "name": "read", "parameters": {"type": "object"}}
            ]},
            {"type": "tool_search"},
            {"type": "x_search", "allowed_x_handles": ["openai"]}
        ],
        "tool_choice": {"type": "tool_search"}
    })
    .to_string();
    let (out, _) = request(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, &body);
    let tools = out["tools"].as_array().unwrap();
    let exec = tools
        .iter()
        .find(|tool| tool["function"]["name"] == "exec")
        .unwrap();
    assert_eq!(
        exec["function"]["parameters"]["properties"]["input"]["type"],
        "string"
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool["function"]["name"] == "fs__read")
    );
    assert!(
        tools
            .iter()
            .any(|tool| tool["function"]["name"] == "tool_search")
    );
    assert!(tools.iter().any(|tool| tool["type"] == "x_search"));
    assert_eq!(out["tool_choice"]["function"]["name"], "tool_search");
}

#[test]
fn chat_to_responses_preserves_builtin_tools() {
    let body = serde_json::json!({
        "model": "m",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [
            {"type": "web_search"},
            {"type": "x_search", "allowed_x_handles": ["openai"]},
            {"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}
        ]
    })
    .to_string();
    let (out, _) = request(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, &body);
    let tools = out["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| tool["type"] == "web_search"));
    let x_search = tools
        .iter()
        .find(|tool| tool["type"] == "x_search")
        .unwrap();
    assert_eq!(x_search["allowed_x_handles"], serde_json::json!(["openai"]));
    let function = tools
        .iter()
        .find(|tool| tool["type"] == "function")
        .unwrap();
    assert_eq!(function["strict"], false);
}

#[test]
fn web_search_maps_between_anthropic_and_responses() {
    let anthropic_body = serde_json::json!({
        "model": "m",
        "max_tokens": 10,
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "web_search_20250305", "name": "web_search"}]
    })
    .to_string();
    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &anthropic_body,
    );
    assert_eq!(responses["tools"][0]["type"], "web_search");

    let responses_body = serde_json::json!({
        "model": "m",
        "input": "hi",
        "tools": [{"type": "web_search"}]
    })
    .to_string();
    let (anthropic, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &responses_body,
    );
    assert_eq!(anthropic["tools"][0]["type"], "web_search_20250305");
    assert_eq!(anthropic["tools"][0]["name"], "web_search");
}

#[test]
fn tool_schema_root_unions_are_normalized_for_anthropic() {
    let body = serde_json::json!({
        "model": "m",
        "input": "hi",
        "tools": [{
            "type": "function",
            "name": "update",
            "parameters": {
                "anyOf": [
                    {"type": "object", "properties": {"a": {"type": "string"}}},
                    {"type": "object", "properties": {"b": {"type": "integer"}}}
                ],
                "properties": {"root": {"type": "boolean"}}
            }
        }]
    })
    .to_string();
    let (out, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    let schema = &out["tools"][0]["input_schema"];
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"].get("a").is_some());
    assert!(schema["properties"].get("b").is_some());
    assert!(schema["properties"].get("root").is_some());
}

#[test]
fn chat_to_anthropic_maps_web_search_to_server_tool() {
    let body = serde_json::json!({
        "model": "m",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "web_search"}]
    })
    .to_string();
    let (out, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert_eq!(out["tools"][0]["type"], "web_search_20250305");
    assert_eq!(out["tools"][0]["name"], "web_search");
}

#[test]
fn responses_to_anthropic_keeps_only_paired_tool_calls() {
    let body = serde_json::json!({
        "model": "m",
        "input": [
            {"type": "message", "role": "user", "content": "hi"},
            {"type": "function_call", "call_id": "call_1", "name": "a", "arguments": "{}"},
            {"type": "function_call", "call_id": "call_2", "name": "b", "arguments": "{}"},
            {"type": "function_call_output", "call_id": "call_1", "output": "ok"},
            {"type": "function_call_output", "call_id": "orphan", "output": "drop"}
        ]
    })
    .to_string();
    let (out, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    let messages = out["messages"].as_array().unwrap();
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    let tool_uses: Vec<&serde_json::Value> = assistant["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|block| block["type"] == "tool_use")
        .collect();
    assert_eq!(tool_uses.len(), 1);
    assert_eq!(tool_uses[0]["id"], "call_1");

    let results: Vec<&serde_json::Value> = messages
        .iter()
        .filter(|message| message["role"] == "user")
        .flat_map(|message| message["content"].as_array().unwrap())
        .filter(|block| block["type"] == "tool_result")
        .collect();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["tool_use_id"], "call_1");
}

#[test]
fn responses_custom_and_tool_search_calls_become_chat_tool_calls() {
    let body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "model": "m",
        "status": "completed",
        "output": [
            {"type": "custom_tool_call", "call_id": "c1", "name": "exec", "input": "ls"},
            {"type": "tool_search_call", "call_id": "c2", "arguments": {"query": "tools"}},
            {"type": "x_search_call", "call_id": "c3", "action": {"query": "news"}}
        ]
    })
    .to_string();
    let (out, _) = response(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, &body);
    let calls = out["choices"][0]["message"]["tool_calls"]
        .as_array()
        .unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls[0]["function"]["name"], "exec");
    assert_eq!(calls[0]["function"]["arguments"], "{\"input\":\"ls\"}");
    assert_eq!(calls[1]["function"]["name"], "tool_search");
    assert_eq!(calls[2]["function"]["name"], "x_search");
}

#[test]
fn responses_custom_tool_history_becomes_anthropic_tool_use() {
    let body = serde_json::json!({
        "model": "m",
        "input": [
            {"type": "message", "role": "user", "content": "hi"},
            {"type": "custom_tool_call", "call_id": "c1", "name": "exec", "input": "ls"},
            {"type": "custom_tool_call_output", "call_id": "c1", "output": "ok"}
        ]
    })
    .to_string();
    let (out, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    let messages = out["messages"].as_array().unwrap();
    let assistant = messages
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap();
    assert_eq!(assistant["content"][0]["type"], "tool_use");
    assert_eq!(assistant["content"][0]["name"], "exec");
    assert_eq!(assistant["content"][0]["input"]["input"], "ls");
    assert!(messages.iter().any(|message| {
        message["content"]
            .as_array()
            .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "tool_result"))
    }));
}

#[test]
fn responses_to_chat_stream_maps_custom_and_tool_search_calls() {
    let jsonl = r#"{"type":"response.created","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"in_progress","output":[]},"sequence_number":0}
{"type":"response.output_item.added","output_index":0,"item":{"type":"custom_tool_call","id":"i1","call_id":"c1","name":"exec"},"sequence_number":1}
{"type":"response.custom_tool_call_input.delta","item_id":"i1","output_index":0,"delta":"ls","sequence_number":2}
{"type":"response.output_item.done","output_index":0,"item":{"type":"custom_tool_call","id":"i1","call_id":"c1","name":"exec","input":"ls"},"sequence_number":3}
{"type":"response.output_item.added","output_index":1,"item":{"type":"tool_search_call","id":"i2","call_id":"c2"},"sequence_number":4}
{"type":"response.output_item.done","output_index":1,"item":{"type":"tool_search_call","id":"i2","call_id":"c2","arguments":{"query":"tools"}},"sequence_number":5}
{"type":"response.completed","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[]},"sequence_number":6}"#;
    let (out, _) = stream_with_report(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, jsonl);
    assert!(out.contains("\"name\":\"exec\""));
    assert!(out.contains("\"arguments\":\"{\\\"input\\\":\\\"ls\\\"}\""));
    assert!(out.contains("\"name\":\"tool_search\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn responses_to_anthropic_stream_maps_custom_and_tool_search_calls() {
    let jsonl = r#"{"type":"response.created","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"in_progress","output":[]},"sequence_number":0}
{"type":"response.output_item.added","output_index":0,"item":{"type":"custom_tool_call","id":"i1","call_id":"c1","name":"exec"},"sequence_number":1}
{"type":"response.custom_tool_call_input.delta","item_id":"i1","output_index":0,"delta":"ls","sequence_number":2}
{"type":"response.output_item.done","output_index":0,"item":{"type":"custom_tool_call","id":"i1","call_id":"c1","name":"exec","input":"ls"},"sequence_number":3}
{"type":"response.output_item.added","output_index":1,"item":{"type":"tool_search_call","id":"i2","call_id":"c2"},"sequence_number":4}
{"type":"response.output_item.done","output_index":1,"item":{"type":"tool_search_call","id":"i2","call_id":"c2","arguments":{"query":"tools"}},"sequence_number":5}
{"type":"response.completed","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[]},"sequence_number":6}"#;
    let (out, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        jsonl,
    );
    assert!(out.contains("\"name\":\"exec\""));
    assert!(out.contains("\"partial_json\":\"{\\\"input\\\":\\\"ls\\\"}\""));
    assert!(out.contains("\"name\":\"tool_search\""));
    assert!(out.contains("event: message_stop"));
}

#[test]
fn chat_to_responses_stream_handles_late_tool_name_and_args_first() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_late","function":{"arguments":"{\"city\":"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"SF\""}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"get_weather","arguments":"}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, _) = stream_with_report(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, jsonl);
    let added = out.find("\"name\":\"get_weather\"").unwrap();
    let first_delta = out.find("response.function_call_arguments.delta").unwrap();
    assert!(
        added < first_delta,
        "arguments emitted before item announced"
    );
    assert!(out.contains("{\\\"city\\\":"));
    assert!(out.contains("\\\"SF\\\""));
    assert!(out.contains("event: response.completed"));
}

#[test]
fn chat_to_responses_stream_emits_empty_tool_args_placeholder() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_empty","type":"function","function":{"name":"noop","arguments":""}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, _) = stream_with_report(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, jsonl);
    assert!(out.contains("\"name\":\"noop\""));
    assert!(out.contains("\"delta\":\"{}\""));
}

#[test]
fn chat_tool_search_call_is_restored_to_responses_tool_search_call() {
    let body = serde_json::json!({
        "id": "c1",
        "object": "chat.completion",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "tool_search", "arguments": "{\"query\":\"tools\"}"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    })
    .to_string();
    let (out, _) = response(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, &body);
    assert_eq!(out["output"][0]["type"], "tool_search_call");
    assert_eq!(out["output"][0]["arguments"]["query"], "tools");
}

#[test]
fn chat_tool_search_stream_is_restored_to_responses_tool_search_call() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"tool_search","arguments":"{\"query\":\"tools\"}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, _) = stream_with_report(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, jsonl);
    assert!(out.contains("\"type\":\"tool_search_call\""));
    assert!(out.contains("\"query\":\"tools\""));
}

#[test]
fn missing_ids_and_empty_choices_are_synthesized() {
    let chat_body = serde_json::json!({
        "model": "m",
        "choices": []
    })
    .to_string();
    let (anthropic, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &chat_body,
    );
    assert_eq!(anthropic["id"], "msg_transcoded");
    assert_eq!(anthropic["content"][0]["type"], "text");
    assert_eq!(anthropic["stop_reason"], "end_turn");

    let responses_body = serde_json::json!({
        "object": "response",
        "model": "m",
        "status": "completed",
        "output": []
    })
    .to_string();
    let (chat, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &responses_body,
    );
    assert_eq!(chat["id"], "chatcmpl_transcoded");
    assert_eq!(chat["choices"][0]["message"]["content"], "");

    let chat_body = serde_json::json!({
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hi"},
            "finish_reason": "stop"
        }]
    })
    .to_string();
    let (responses, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &chat_body,
    );
    assert_eq!(responses["id"], "resp_transcoded");
    assert_eq!(responses["output"][0]["type"], "message");
}

#[test]
fn refusal_and_token_details_are_preserved() {
    let responses_body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "model": "m",
        "status": "completed",
        "output": [{
            "type": "message",
            "id": "msg_1",
            "role": "assistant",
            "status": "completed",
            "content": [{"type": "refusal", "refusal": "cannot help"}]
        }],
        "usage": {
            "input_tokens": 10,
            "output_tokens": 5,
            "total_tokens": 15,
            "input_tokens_details": {"cached_tokens": 2, "audio_tokens": 1},
            "output_tokens_details": {
                "reasoning_tokens": 3,
                "audio_tokens": 1,
                "accepted_prediction_tokens": 2,
                "rejected_prediction_tokens": 1
            }
        }
    })
    .to_string();
    let (chat, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &responses_body,
    );
    assert_eq!(chat["choices"][0]["message"]["refusal"], "cannot help");
    assert!(
        chat["choices"][0]["message"]["content"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(chat["usage"]["prompt_tokens_details"]["audio_tokens"], 1);
    assert_eq!(
        chat["usage"]["completion_tokens_details"]["reasoning_tokens"],
        3
    );
    assert_eq!(
        chat["usage"]["completion_tokens_details"]["accepted_prediction_tokens"],
        2
    );

    let chat_body = serde_json::json!({
        "id": "c1",
        "object": "chat.completion",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hi"},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15,
            "prompt_tokens_details": {"cached_tokens": 2, "audio_tokens": 1},
            "completion_tokens_details": {
                "reasoning_tokens": 3,
                "audio_tokens": 1,
                "accepted_prediction_tokens": 2,
                "rejected_prediction_tokens": 1
            }
        }
    })
    .to_string();
    let (responses, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &chat_body,
    );
    assert_eq!(
        responses["usage"]["input_tokens_details"]["audio_tokens"],
        1
    );
    assert_eq!(
        responses["usage"]["output_tokens_details"]["reasoning_tokens"],
        3
    );
    assert_eq!(
        responses["usage"]["output_tokens_details"]["accepted_prediction_tokens"],
        2
    );
}

#[test]
fn responses_read_tool_drops_empty_pages() {
    let body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "model": "m",
        "status": "completed",
        "output": [{
            "type": "function_call",
            "call_id": "c1",
            "name": "Read",
            "arguments": "{\"file_path\":\"/tmp/a.py\",\"pages\":\"\"}"
        }]
    })
    .to_string();
    let (out, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert_eq!(out["content"][0]["type"], "tool_use");
    assert!(out["content"][0]["input"].get("pages").is_none());
    assert_eq!(out["content"][0]["input"]["file_path"], "/tmp/a.py");
}

#[test]
fn anthropic_billing_header_is_filtered_from_system() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "system": [
            {"type": "text", "text": "x-anthropic-billing-header: secret"},
            {"type": "text", "text": "real instructions"}
        ],
        "messages": [{"role": "user", "content": "hi"}]
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    let system = &chat["messages"][0];
    assert_eq!(system["role"], "system");
    assert_eq!(system["content"], "real instructions");

    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert!(responses.get("instructions").is_none());
    assert_eq!(responses["input"][0]["role"], "developer");
    assert_eq!(
        responses["input"][0]["content"][0]["text"],
        "real instructions"
    );
}

#[test]
fn flattened_namespace_names_are_restored_in_responses() {
    let body = serde_json::json!({
        "id": "c1",
        "object": "chat.completion",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "fs__read", "arguments": "{}"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    })
    .to_string();
    let (out, _) = response(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, &body);
    assert_eq!(out["output"][0]["type"], "function_call");
    assert_eq!(out["output"][0]["namespace"], "fs");
    assert_eq!(out["output"][0]["name"], "read");
}

#[test]
fn response_context_restores_custom_and_namespace_calls() {
    use solidway_transcode::registry::{response_context, response_with_context};

    let request = serde_json::json!({
        "model": "m",
        "input": "hi",
        "tools": [
            {"type": "custom", "name": "exec"},
            {"type": "namespace", "name": "fs", "tools": [
                {"type": "function", "name": "read", "parameters": {"type": "object"}}
            ]}
        ]
    })
    .to_string();
    let context = response_context(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        request.as_bytes(),
    );
    let response_body = serde_json::json!({
        "id": "c1",
        "object": "chat.completion",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "exec", "arguments": "{\"input\":\"ls\"}"}},
                    {"id": "c2", "type": "function", "function": {"name": "fs__read", "arguments": "{}"}}
                ]
            },
            "finish_reason": "tool_calls"
        }]
    })
    .to_string();
    let plan = Plan {
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::OPENAI_CHAT,
        policy: TranscodePolicy::BestEffort,
    };
    let output = response_with_context(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        response_body.as_bytes(),
        &plan,
        &context,
    )
    .unwrap();
    let out: serde_json::Value = serde_json::from_slice(&output.body).unwrap();
    assert_eq!(out["output"][0]["type"], "custom_tool_call");
    assert_eq!(out["output"][0]["input"], "ls");
    assert_eq!(out["output"][1]["type"], "function_call");
    assert_eq!(out["output"][1]["namespace"], "fs");
    assert_eq!(out["output"][1]["name"], "read");
}

#[test]
fn response_context_restores_custom_and_namespace_stream_calls() {
    use solidway_transcode::registry::{response_context, stream_translator_with_context};

    let request = serde_json::json!({
        "model": "m",
        "input": "hi",
        "tools": [
            {"type": "custom", "name": "exec"},
            {"type": "namespace", "name": "fs", "tools": [
                {"type": "function", "name": "read", "parameters": {"type": "object"}}
            ]}
        ]
    })
    .to_string();
    let context = response_context(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        request.as_bytes(),
    );
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"exec","arguments":"{\"input\":\"ls\"}"}},{"index":1,"id":"c2","type":"function","function":{"name":"fs__read","arguments":"{}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let plan = Plan {
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::OPENAI_CHAT,
        policy: TranscodePolicy::BestEffort,
    };
    let mut translator = stream_translator_with_context(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &plan,
        &context,
    )
    .unwrap();
    let input: String = jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("data: {line}\n\n"))
        .collect();
    let mut output = translator.feed(input.as_bytes()).unwrap();
    output.extend(translator.finish().unwrap());
    let out = String::from_utf8(output).unwrap();
    assert!(out.contains("\"type\":\"custom_tool_call\""));
    assert!(out.contains("\"input\":\"ls\""));
    assert!(out.contains("\"namespace\":\"fs\""));
    assert!(out.contains("\"name\":\"read\""));
}

#[test]
fn empty_bodies_and_null_synthesize_responses() {
    let cases = [
        (
            ProtocolId::OPENAI_CHAT,
            ProtocolId::ANTHROPIC_MESSAGES,
            "chatcmpl_transcoded",
        ),
        (
            ProtocolId::ANTHROPIC_MESSAGES,
            ProtocolId::OPENAI_CHAT,
            "msg_transcoded",
        ),
        (
            ProtocolId::OPENAI_CHAT,
            ProtocolId::OPENAI_RESPONSES,
            "chatcmpl_transcoded",
        ),
        (
            ProtocolId::OPENAI_RESPONSES,
            ProtocolId::OPENAI_CHAT,
            "resp_transcoded",
        ),
        (
            ProtocolId::ANTHROPIC_MESSAGES,
            ProtocolId::OPENAI_RESPONSES,
            "msg_transcoded",
        ),
        (
            ProtocolId::OPENAI_RESPONSES,
            ProtocolId::ANTHROPIC_MESSAGES,
            "resp_transcoded",
        ),
    ];
    for (src, dst, expected_id) in cases {
        for body in ["", "null"] {
            let (out, _) = response(src, dst, body);
            assert_eq!(out["id"], expected_id, "{src} -> {dst} body={body:?}");
        }
    }
}

#[test]
fn server_tool_shapes_are_preserved() {
    let chat_body = serde_json::json!({
        "model": "m",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "web_search", "search_context_size": "high"}]
    })
    .to_string();
    let (responses, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &chat_body,
    );
    assert_eq!(responses["tools"][0]["type"], "web_search");
    assert_eq!(responses["tools"][0]["search_context_size"], "high");

    let responses_body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "model": "m",
        "status": "completed",
        "output": [{
            "type": "web_search_call",
            "call_id": "c1",
            "action": {"query": "news"}
        }]
    })
    .to_string();
    let (anthropic, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &responses_body,
    );
    assert_eq!(anthropic["content"][0]["type"], "server_tool_use");
    assert_eq!(anthropic["content"][0]["name"], "web_search");
    assert_eq!(anthropic["content"][0]["input"]["query"], "news");

    let anthropic_request = serde_json::json!({
        "model": "m",
        "max_tokens": 10,
        "messages": [{
            "role": "assistant",
            "content": [{
                "type": "server_tool_use",
                "id": "s1",
                "name": "web_search",
                "input": {"query": "news"}
            }]
        }]
    })
    .to_string();
    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &anthropic_request,
    );
    assert!(
        responses["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "function_call" && item["name"] == "web_search")
    );
}

#[test]
fn anthropic_thinking_is_dropped_without_tool_calls() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "secret", "signature": "sig"},
                {"type": "text", "text": "answer"}
            ]
        }]
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert!(chat["messages"][0].get("reasoning_content").is_none());
    assert_eq!(chat["messages"][0]["content"], "answer");
    assert_eq!(chat["max_completion_tokens"], 128);
}

#[test]
fn anthropic_text_array_user_content_folds_to_string() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "first"},
                {"type": "text", "text": "second"}
            ]
        }]
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert_eq!(chat["messages"][0]["content"], "first\n\nsecond");
}

#[test]
fn anthropic_thinking_without_signature_is_dropped_for_responses() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "messages": [{
            "role": "assistant",
            "content": [
                {"type": "thinking", "thinking": "secret"},
                {"type": "text", "text": "answer"}
            ]
        }]
    })
    .to_string();
    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert!(
        !responses["input"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["type"] == "reasoning")
    );
}

#[test]
fn responses_reasoning_summary_only_is_dropped_for_anthropic() {
    let body = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "reasoning",
            "summary": [{"type": "summary_text", "text": "summary only"}]
        }, {
            "type": "message",
            "role": "user",
            "content": "hi"
        }]
    })
    .to_string();
    let (anthropic, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert!(
        !anthropic["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_array()
                .is_some_and(|blocks| blocks.iter().any(|block| block["type"] == "thinking")))
    );
}

#[test]
fn responses_agent_message_becomes_user_and_empty_media_is_skipped() {
    let body = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "message",
            "role": "agent",
            "content": [{"type": "input_text", "text": "hi"}]
        }]
    })
    .to_string();
    let (chat, _) = request(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, &body);
    assert_eq!(chat["messages"][0]["role"], "user");

    let body = serde_json::json!({
        "model": "m",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,"}},
                {"type": "file", "file": {}}
            ]
        }]
    })
    .to_string();
    let (responses, _) = request(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, &body);
    assert!(
        !responses["input"][0]["content"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .any(|part| part["type"] == "input_image" || part["type"] == "input_file")
    );
}

#[test]
fn anthropic_chat_defaults_reasoning_effort_for_reasoning_models() {
    let body = serde_json::json!({
        "model": "gpt-5.4",
        "max_tokens": 100,
        "messages": [{"role": "user", "content": "hi"}]
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert_eq!(chat["reasoning_effort"], "medium");
}

#[test]
fn chat_legacy_function_pairs_by_name_when_call_id_missing() {
    let body = serde_json::json!({
        "model": "m",
        "messages": [
            {"role": "assistant", "content": "", "function_call": {"name": "legacy", "arguments": "{\"a\":1}"}},
            {"role": "function", "name": "legacy", "content": "ok"}
        ]
    })
    .to_string();
    let (responses, _) = request(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, &body);
    let input = responses["input"].as_array().unwrap();
    let call = input
        .iter()
        .find(|item| item["type"] == "function_call")
        .unwrap();
    let output = input
        .iter()
        .find(|item| item["type"] == "function_call_output")
        .unwrap();
    assert_eq!(call["call_id"], "call_legacy_1");
    assert_eq!(output["call_id"], "call_legacy_1");
}

#[test]
fn responses_agent_message_without_text_is_skipped_for_chat() {
    let body = serde_json::json!({
        "model": "m",
        "input": [
            {"type": "message", "role": "agent", "content": []},
            {"type": "message", "role": "user", "content": "hi"}
        ]
    })
    .to_string();
    let (chat, _) = request(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, &body);
    let messages = chat["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[test]
fn responses_to_anthropic_stream_preserves_thinking_signature() {
    let jsonl = r#"{"type":"response.created","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"in_progress","output":[]},"sequence_number":0}
{"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs1","encrypted_content":"anthropic-thinking-v1:{\"thinking\":\"plan\",\"signature\":\"sig\"}"},"sequence_number":1}
{"type":"response.reasoning_summary_text.delta","item_id":"rs1","output_index":0,"summary_index":0,"delta":"plan","sequence_number":2}
{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs1","encrypted_content":"anthropic-thinking-v1:{\"thinking\":\"plan\",\"signature\":\"sig\"}"},"sequence_number":3}
{"type":"response.completed","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[]},"sequence_number":4}"#;
    let (out, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        jsonl,
    );
    assert!(out.contains("\"signature\":\"sig\""));
    assert!(out.contains("\"type\":\"signature_delta\""));
}

#[test]
fn zero_token_details_are_omitted_and_x_search_choice_is_string() {
    let chat_body = serde_json::json!({
        "id": "c1",
        "object": "chat.completion",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "hi"},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15,
            "completion_tokens_details": {"reasoning_tokens": 0}
        }
    })
    .to_string();
    let (responses, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &chat_body,
    );
    assert!(
        responses["usage"]["output_tokens_details"]
            .get("reasoning_tokens")
            .is_none()
    );

    let responses_body = serde_json::json!({
        "model": "m",
        "input": "hi",
        "tools": [{"type": "x_search"}],
        "tool_choice": {"type": "x_search"}
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &responses_body,
    );
    assert_eq!(chat["tool_choice"], "x_search");
}

#[test]
fn unknown_responses_items_keep_recognizable_text() {
    let response_body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "model": "m",
        "status": "completed",
        "output": [{
            "type": "unknown_item",
            "id": "x1",
            "content": [{"type": "output_text", "text": "keep me"}]
        }]
    })
    .to_string();
    let (anthropic, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &response_body,
    );
    assert!(
        anthropic["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|block| block["type"] == "text" && block["text"] == "keep me")
    );

    let request_body = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "unknown_item",
            "content": "keep me"
        }]
    })
    .to_string();
    let (anthropic, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &request_body,
    );
    assert!(
        anthropic["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_array()
                .is_some_and(|blocks| blocks.iter().any(|block| block["text"] == "keep me")))
    );
}

#[test]
fn chat_to_responses_custom_stream_emits_custom_input_done() {
    use solidway_transcode::registry::{ResponseContext, stream_translator_with_context};

    let mut context = ResponseContext::default();
    context.custom_tools.insert("exec".to_string());
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"exec","arguments":"{\"input\":\"ls\"}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let plan = Plan {
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::OPENAI_CHAT,
        policy: TranscodePolicy::BestEffort,
    };
    let mut translator = stream_translator_with_context(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &plan,
        &context,
    )
    .unwrap();
    let input: String = jsonl
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format!("data: {line}\n\n"))
        .collect();
    let mut output = translator.feed(input.as_bytes()).unwrap();
    output.extend(translator.finish().unwrap());
    let out = String::from_utf8(output).unwrap();
    assert!(out.contains("response.custom_tool_call_input.done"));
    assert!(out.contains("\"input\":\"ls\""));
}

#[test]
fn responses_to_chat_stream_uses_function_arguments_done_without_deltas() {
    let jsonl = r#"{"type":"response.created","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"in_progress","output":[]},"sequence_number":0}
{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"i1","call_id":"c1","name":"f"},"sequence_number":1}
{"type":"response.function_call_arguments.done","item_id":"i1","output_index":0,"call_id":"c1","name":"f","arguments":"{\"a\":1}","sequence_number":2}
{"type":"response.completed","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[]},"sequence_number":3}"#;
    let (out, _) = stream_with_report(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, jsonl);
    assert!(out.contains("\"arguments\":\"{\\\"a\\\":1}\""));
    assert!(out.contains("data: [DONE]"));
}

#[test]
fn responses_to_anthropic_drops_assistant_with_only_unknown_parts() {
    let body = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "unknown_part", "text": "ignored"}]
        }]
    })
    .to_string();
    let (out, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert!(
        !out["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["role"] == "assistant")
    );
}

#[test]
fn chat_to_responses_accepts_flattened_function_tool_choice() {
    let body = serde_json::json!({
        "model": "m",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}],
        "tool_choice": {"type": "function", "name": "f"}
    })
    .to_string();
    let (out, _) = request(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, &body);
    assert_eq!(out["tool_choice"]["type"], "function");
    assert_eq!(out["tool_choice"]["name"], "f");
}

#[test]
fn responses_stream_recovers_text_from_done_event() {
    let jsonl = r#"{"type":"response.created","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"in_progress","output":[]},"sequence_number":0}
{"type":"response.output_text.done","item_id":"msg_1","output_index":0,"content_index":0,"text":"Hello","sequence_number":1}
{"type":"response.completed","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[]},"sequence_number":2}"#;
    let (chat, _) =
        stream_with_report(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, jsonl);
    assert!(chat.contains("\"content\":\"Hello\""));

    let (anthropic, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        jsonl,
    );
    assert!(anthropic.contains("\"type\":\"text_delta\""));
    assert!(anthropic.contains("\"text\":\"Hello\""));
}

#[test]
fn responses_stream_recovers_text_from_terminal_output() {
    let jsonl = r#"{"type":"response.created","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"in_progress","output":[]},"sequence_number":0}
{"type":"response.completed","response":{"id":"r","object":"response","created_at":1,"model":"m","status":"completed","output":[{"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Hello"}]}]},"sequence_number":1}"#;
    let (chat, _) =
        stream_with_report(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, jsonl);
    assert!(chat.contains("\"content\":\"Hello\""));

    let (anthropic, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        jsonl,
    );
    assert!(anthropic.contains("\"type\":\"text_delta\""));
    assert!(anthropic.contains("\"text\":\"Hello\""));
}

#[test]
fn responses_promotes_tool_search_discoveries_and_additional_tools() {
    let body = serde_json::json!({
        "model": "m",
        "tools": [
            {"type": "tool_search"},
            {"type": "additional_tools", "tools": [
                {"type": "function", "name": "extra", "parameters": {"type": "object"}}
            ]}
        ],
        "input": [
            {"type": "tool_search_output", "tools": [
                {"type": "function", "name": "new_tool", "parameters": {"type": "object"}}
            ]},
            {"type": "message", "role": "user", "content": "hi"}
        ]
    })
    .to_string();
    let (chat, _) = request(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT, &body);
    let names: Vec<&str> = chat["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    assert!(names.contains(&"tool_search"));
    assert!(names.contains(&"new_tool"));
    assert!(names.contains(&"extra"));
}

#[test]
fn responses_agent_message_becomes_user_for_anthropic() {
    let body = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "message",
            "role": "agent",
            "content": "hi"
        }]
    })
    .to_string();
    let (anthropic, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert_eq!(anthropic["messages"][0]["role"], "user");
    assert_eq!(anthropic["messages"][0]["content"][0]["text"], "hi");
}

#[test]
fn web_search_citations_and_results_are_mapped() {
    let responses_body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "model": "m",
        "status": "completed",
        "output": [
            {
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "status": "completed",
                "content": [{
                    "type": "output_text",
                    "text": "hi",
                    "annotations": [{"type": "url_citation", "url": "https://example.com"}]
                }]
            },
            {
                "type": "web_search_call",
                "id": "ws1",
                "call_id": "c1",
                "action": {"query": "news"},
                "results": [{"title": "example"}]
            }
        ]
    })
    .to_string();
    let (anthropic, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &responses_body,
    );
    let blocks = anthropic["content"].as_array().unwrap();
    let text = blocks.iter().find(|block| block["type"] == "text").unwrap();
    assert_eq!(text["citations"][0]["url"], "https://example.com");
    assert!(
        blocks
            .iter()
            .any(|block| block["type"] == "web_search_tool_result")
    );

    let (chat, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &responses_body,
    );
    assert_eq!(
        chat["choices"][0]["message"]["annotations"][0]["url"],
        "https://example.com"
    );
    assert!(chat["choices"][0]["message"].get("tool_calls").is_none());
}

#[test]
fn x_search_results_are_kept_in_chat_arguments() {
    let body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "created_at": 1,
        "model": "m",
        "status": "completed",
        "output": [{
            "type": "x_search_call",
            "call_id": "c1",
            "action": {"query": "news"},
            "results": [{"title": "example"}]
        }]
    })
    .to_string();
    let (chat, _) = response(ProtocolId::OPENAI_CHAT, ProtocolId::OPENAI_RESPONSES, &body);
    let call = &chat["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(call["function"]["name"], "x_search");
    let arguments: serde_json::Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(arguments["results"][0]["title"], "example");
}

#[test]
fn chat_annotations_become_anthropic_citations() {
    let body = serde_json::json!({
        "id": "c1",
        "object": "chat.completion",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "hi",
                    "annotations": [{"type": "url_citation", "url": "https://example.com"}]
                }]
            },
            "finish_reason": "stop"
        }]
    })
    .to_string();
    let (anthropic, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert_eq!(
        anthropic["content"][0]["citations"][0]["url"],
        "https://example.com"
    );
}

#[test]
fn anthropic_text_document_and_audio_are_mapped() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 10,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "document",
                "source": {"type": "text", "media_type": "text/plain", "data": "doc text"}
            }]
        }]
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert_eq!(chat["messages"][0]["content"], "doc text");
    let (responses, _) = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert_eq!(responses["input"][0]["content"][0]["text"], "doc text");

    let chat_audio = serde_json::json!({
        "model": "m",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": {"data": "AAAA", "format": "wav"}
            }]
        }]
    })
    .to_string();
    let (responses, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &chat_audio,
    );
    assert_eq!(
        responses["input"][0]["content"][0]["input_audio"]["format"],
        "wav"
    );

    let responses_audio = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": {"data": "AAAA", "format": "wav"}
            }]
        }]
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &responses_audio,
    );
    assert_eq!(
        chat["messages"][0]["content"][0]["input_audio"]["format"],
        "wav"
    );
}

#[test]
fn allowed_tools_tool_choice_passes_through() {
    let choice = serde_json::json!({
        "type": "allowed_tools",
        "allowed_tools": {"mode": "auto", "tools": [{"type": "function", "name": "f"}]}
    });
    let chat_body = serde_json::json!({
        "model": "m",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}],
        "tool_choice": choice
    })
    .to_string();
    let (responses, _) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &chat_body,
    );
    assert_eq!(responses["tool_choice"]["type"], "allowed_tools");

    let responses_body = serde_json::json!({
        "model": "m",
        "input": "hi",
        "tools": [{"type": "function", "name": "f", "parameters": {"type": "object"}}],
        "tool_choice": choice
    })
    .to_string();
    let (chat, _) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &responses_body,
    );
    assert_eq!(chat["tool_choice"]["type"], "allowed_tools");
}

#[test]
fn error_extras_and_retry_after_are_preserved() {
    let responses_error = r#"{"error":{"message":"slow down","type":"rate_limit_error","code":"rate_limited","retry_after_ms":1500}}"#;
    let (chat, _) = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        responses_error,
    );
    assert_eq!(chat["error"]["retry_after_ms"], 1500);

    let chat_error = r#"{"error":{"message":"slow down","type":"rate_limit_error","code":"rate_limited","param":"model","retry_after":2}}"#;
    let (anthropic, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        chat_error,
    );
    assert_eq!(anthropic["error"]["retry_after"], 2);
    assert_eq!(anthropic["error"]["code"], "rate_limited");
    assert_eq!(anthropic["error"]["param"], "model");

    let anthropic_error = r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down","retry_after":3}}"#;
    let (responses, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        anthropic_error,
    );
    assert_eq!(responses["error"]["retry_after"], 3);
}

#[test]
fn chat_to_anthropic_stream_orders_interleaved_tool_arguments() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c0","type":"function","function":{"name":"first","arguments":"{\"a\":"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"c1","type":"function","function":{"name":"second","arguments":"{\"b\":"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"1}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"2}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, _) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        jsonl,
    );
    let mut starts = Vec::new();
    let mut arguments: std::collections::BTreeMap<u64, String> = Default::default();
    for line in out.lines() {
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        let value: serde_json::Value = serde_json::from_str(data).unwrap();
        match value["type"].as_str() {
            Some("content_block_start") => {
                starts.push(value["index"].as_u64().unwrap());
            }
            Some("content_block_delta") => {
                if let Some(partial) = value["delta"]["partial_json"].as_str() {
                    arguments
                        .entry(value["index"].as_u64().unwrap())
                        .or_default()
                        .push_str(partial);
                }
            }
            _ => {}
        }
    }
    assert_eq!(starts, vec![0, 1]);
    assert_eq!(arguments.get(&0).unwrap(), "{\"a\":1}");
    assert_eq!(arguments.get(&1).unwrap(), "{\"b\":2}");
}

#[test]
fn responses_web_search_maps_to_anthropic_server_tool_pairs() {
    let body = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "status": "completed",
        "model": "m",
        "output": [{
            "type": "web_search_call",
            "id": "ws_1",
            "status": "completed",
            "action": {"type": "search", "query": "rust"},
            "results": [{"type": "web_search_result", "url": "https://example.com", "title": "Example"}]
        }]
    })
    .to_string();
    let (out, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert_eq!(out["content"][0]["type"], "server_tool_use");
    assert_eq!(out["content"][0]["id"], "srvtoolu_ws_1");
    assert_eq!(out["content"][0]["name"], "web_search");
    assert_eq!(out["content"][0]["input"]["query"], "rust");
    assert_eq!(out["content"][1]["type"], "web_search_tool_result");
    assert_eq!(out["content"][1]["tool_use_id"], "srvtoolu_ws_1");
    assert_eq!(
        out["content"][1]["content"][0]["url"],
        "https://example.com"
    );
}

#[test]
fn anthropic_server_tool_pairs_map_to_responses_web_search() {
    let body = serde_json::json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "m",
        "content": [
            {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": {"query": "rust"}},
            {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_1", "content": [
                {"type": "web_search_result", "url": "https://example.com", "title": "Example"}
            ]}
        ],
        "stop_reason": "end_turn"
    })
    .to_string();
    let (out, _) = response(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        &body,
    );
    assert_eq!(out["output"][0]["type"], "web_search_call");
    assert_eq!(out["output"][0]["id"], "srvtoolu_1");
    assert_eq!(out["output"][0]["action"]["query"], "rust");
    assert_eq!(out["output"][0]["results"][0]["url"], "https://example.com");
}

#[test]
fn responses_text_annotations_become_anthropic_citations() {
    let body = serde_json::json!({
        "id": "resp_1",
        "status": "completed",
        "model": "m",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "answer",
                "annotations": [{"type": "url_citation", "url": "https://example.com", "title": "Example"}]
            }]
        }]
    })
    .to_string();
    let (out, _) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &body,
    );
    assert_eq!(
        out["content"][0]["citations"][0]["url"],
        "https://example.com"
    );
}

#[test]
fn responses_web_search_stream_emits_anthropic_result_block() {
    let jsonl = r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"web_search_call","id":"ws_1","status":"in_progress","action":{"type":"search","query":"rust"}},"sequence_number":1}
{"type":"response.output_item.done","output_index":0,"item":{"type":"web_search_call","id":"ws_1","status":"completed","action":{"type":"search","query":"rust"},"results":[{"type":"web_search_result","url":"https://example.com","title":"Example"}]},"sequence_number":2}
{"type":"response.completed","response":{"id":"r1","status":"completed","output":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}},"sequence_number":3}"#;
    let out = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        jsonl,
    );
    assert!(out.contains("\"type\":\"server_tool_use\""));
    assert!(out.contains("\"id\":\"srvtoolu_ws_1\""));
    assert!(out.contains("\"type\":\"web_search_tool_result\""));
    assert!(out.contains("\"url\":\"https://example.com\""));
}

#[test]
fn anthropic_server_tool_stream_emits_responses_web_search() {
    let jsonl = r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"m","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}
{"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{}}}
{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"rust\"}"}}
{"type":"content_block_stop","index":0}
{"type":"content_block_start","index":1,"content_block":{"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[{"type":"web_search_result","url":"https://example.com","title":"Example"}]}}
{"type":"content_block_stop","index":1}
{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":1}}
{"type":"message_stop"}"#;
    let out = stream(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        jsonl,
    );
    assert!(out.contains("\"type\":\"web_search_call\""));
    assert!(out.contains("\"id\":\"srvtoolu_1\""));
    assert!(out.contains("\"query\":\"rust\""));
    assert!(out.contains("\"url\":\"https://example.com\""));
    assert!(!out.contains("response.function_call_arguments.delta"));
}

#[test]
fn invalid_audio_is_rejected_instead_of_forwarded() {
    let chat_body = serde_json::json!({
        "model": "m",
        "messages": [{
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": {"data": "AAAA", "format": "ogg"}
            }]
        }]
    })
    .to_string();
    let (responses, report) = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &chat_body,
    );
    assert!(!responses.to_string().contains("input_audio"));
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.detail.contains("input_audio"))
    );

    let responses_body = serde_json::json!({
        "model": "m",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_audio",
                "input_audio": {"data": "", "format": "wav"}
            }]
        }]
    })
    .to_string();
    let (chat, report) = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &responses_body,
    );
    assert!(!chat.to_string().contains("input_audio"));
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.detail.contains("input_audio"))
    );
}

#[test]
fn chat_to_anthropic_stream_name_never_arrives_warns_and_emits_empty_name() {
    let jsonl = r#"{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_x","function":{"arguments":"{}"}}]},"finish_reason":null}]}
{"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
    let (out, report) = stream_with_report(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        jsonl,
    );
    assert!(out.contains("\"name\":\"\""));
    assert!(out.contains("\"id\":\"call_x\""));
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.detail.contains("never provided a name"))
    );
}

#[test]
fn chat_to_anthropic_generates_missing_tool_id() {
    let body = serde_json::json!({
        "id": "chatcmpl_1",
        "object": "chat.completion",
        "created": 1,
        "model": "m",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "type": "function",
                    "function": {"name": "f", "arguments": "{}"}
                }]
            },
            "finish_reason": "tool_calls"
        }]
    })
    .to_string();
    let (out, report) = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &body,
    );
    assert_eq!(out["content"][0]["type"], "tool_use");
    assert_eq!(out["content"][0]["id"], "toolu_0");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.detail.contains("generated one"))
    );
}

#[test]
fn stream_errors_preserve_provider_extras() {
    let responses_error = r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow","code":"rate_limited","retry_after_ms":1500}}"#;
    let out = stream(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        responses_error,
    );
    assert!(out.contains("\"retry_after_ms\":1500"));
    assert!(out.contains("\"code\":\"rate_limited\""));

    let chat_error = r#"{"error":{"message":"slow","type":"rate_limit_error","code":"rate_limited","param":"model","retry_after":2}}"#;
    let out = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        chat_error,
    );
    assert!(out.contains("\"retry_after\":2"));
    assert!(out.contains("\"code\":\"rate_limited\""));
    assert!(out.contains("\"param\":\"model\""));
}
