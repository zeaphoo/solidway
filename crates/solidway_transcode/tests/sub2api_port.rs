//! 从 sub2api `internal/pkg/apicompat` 直接移植的高价值行为测试。
//!
//! 只移植纯协议转换部分；HTTP / 账号 / 路由 / 计费测试不属于本 crate。

use serde_json::{Value, json};
use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, direction};
use solidway_transcode::report::TranscodePolicy;

fn plan(src: ProtocolId, dst: ProtocolId) -> Plan {
    Plan {
        src,
        dst,
        policy: TranscodePolicy::BestEffort,
    }
}

fn request(src: ProtocolId, dst: ProtocolId, body: Value) -> Value {
    let raw = body.to_string();
    let out = direction(src, dst)
        .expect("direction")
        .request(raw.as_bytes(), &plan(src, dst))
        .unwrap();
    serde_json::from_slice(&out.body).unwrap()
}

fn response(src: ProtocolId, dst: ProtocolId, body: Value) -> Value {
    let raw = body.to_string();
    let out = direction(src, dst)
        .expect("direction")
        .response(raw.as_bytes(), &plan(src, dst))
        .unwrap();
    serde_json::from_slice(&out.body).unwrap()
}

fn stream(src: ProtocolId, dst: ProtocolId, lines: &[Value]) -> Vec<Value> {
    let input: String = lines
        .iter()
        .map(|line| format!("data: {line}\n\n"))
        .collect();
    let mut translator =
        solidway_transcode::registry::stream_translator(src, dst, &plan(src, dst)).unwrap();
    let mut out = translator.feed(input.as_bytes()).unwrap();
    out.extend(translator.finish().unwrap());
    String::from_utf8(out)
        .unwrap()
        .split("\n\n")
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

fn chat_request(messages: Value) -> Value {
    json!({"model": "gpt-4o", "messages": messages})
}

// ---------------------------------------------------------------------------
// Chat -> Responses request
// ---------------------------------------------------------------------------

#[test]
fn port_chat_to_responses_basic_text_and_system() {
    let out = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"}
            ]
        }),
    );
    assert_eq!(out["model"], "gpt-4o");
    assert_eq!(out["stream"], true);
    assert_eq!(out["store"], false);
    assert_eq!(out["input"][0]["type"], "message");
    assert_eq!(out["input"][0]["role"], "system");
    assert_eq!(out["input"][0]["content"], "You are helpful.");
    assert_eq!(out["input"][1]["role"], "user");
    assert_eq!(out["input"][1]["content"], "Hello");
}

#[test]
fn port_chat_to_responses_reasoning_and_tool_items() {
    let out = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "step-5-preview",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Check the directory"},
                {
                    "role": "assistant",
                    "content": "I will check.",
                    "reasoning_content": "Need to inspect the directory.",
                    "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "bash", "arguments": "{\"cmd\":\"pwd\"}"}}]
                },
                {"role": "tool", "tool_call_id": "call_1", "content": "/tmp"},
                {"role": "assistant", "reasoning_content": "The tool returned /tmp."},
                {"role": "user", "content": "Continue"}
            ]
        }),
    );
    let items = out["input"].as_array().unwrap();
    assert_eq!(items.len(), 7);
    let expected = [
        ("message", "system"),
        ("message", "user"),
        ("message", "assistant"),
        ("function_call", ""),
        ("function_call_output", ""),
        ("message", "assistant"),
        ("message", "user"),
    ];
    for (item, (typ, role)) in items.iter().zip(expected) {
        assert_eq!(item["type"], typ);
        if !role.is_empty() {
            assert_eq!(item["role"], role);
        }
    }
    assert_eq!(
        items[2]["content"][0]["text"],
        "<thinking>Need to inspect the directory.</thinking>\nI will check."
    );
    assert_eq!(
        items[5]["content"][0]["text"],
        "<thinking>The tool returned /tmp.</thinking>"
    );
    assert_eq!(items[3]["call_id"], "call_1");
    assert_eq!(items[4]["call_id"], "call_1");
}

#[test]
fn port_chat_to_responses_tool_calls_and_strict() {
    let out = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "user", "content": "Call the function"},
                {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "ping", "arguments": "{\"host\":\"example.com\"}"}}]},
                {"role": "tool", "tool_call_id": "call_1", "content": "pong"}
            ],
            "tools": [{"type": "function", "function": {"name": "ping", "description": "Ping a host", "parameters": {"type": "object"}}}]
        }),
    );
    let items = out["input"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[1]["type"], "function_call");
    assert_eq!(items[1]["call_id"], "call_1");
    assert_eq!(items[1]["name"], "ping");
    assert_eq!(items[2]["type"], "function_call_output");
    assert_eq!(items[2]["call_id"], "call_1");
    assert_eq!(items[2]["output"], "pong");
    assert_eq!(out["tools"][0]["type"], "function");
    assert_eq!(out["tools"][0]["name"], "ping");
    assert_eq!(out["tools"][0]["strict"], false);

    let explicit = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"type": "function", "function": {"name": "ping", "strict": true, "parameters": {"type": "object"}}}]
        }),
    );
    assert_eq!(explicit["tools"][0]["strict"], true);
}

#[test]
fn port_chat_to_responses_max_tokens_floor_and_preference() {
    let floor = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({"model": "gpt-4o", "max_tokens": 1, "messages": [{"role": "user", "content": "Hi"}]}),
    );
    assert_eq!(floor["max_output_tokens"], 128);

    let preferred = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({"model": "gpt-4o", "max_tokens": 100, "max_completion_tokens": 500, "messages": [{"role": "user", "content": "Hi"}]}),
    );
    assert_eq!(preferred["max_output_tokens"], 500);
}

#[test]
fn port_chat_to_responses_reasoning_and_response_format() {
    let reasoning = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({"model": "gpt-4o", "reasoning_effort": "high", "messages": [{"role": "user", "content": "Hi"}]}),
    );
    assert_eq!(reasoning["reasoning"]["effort"], "high");
    assert_eq!(reasoning["reasoning"]["summary"], "auto");

    let json_object = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({"model": "gpt-4o", "response_format": {"type": "json_object"}, "messages": [{"role": "user", "content": "JSON"}]}),
    );
    assert_eq!(json_object["text"]["format"]["type"], "json_object");

    let json_schema = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "response_format": {"type": "json_schema", "json_schema": {"name": "answer", "schema": {"type": "object", "properties": {"ok": {"type": "boolean"}}, "required": ["ok"]}, "strict": true}},
            "messages": [{"role": "user", "content": "JSON"}]
        }),
    );
    assert_eq!(json_schema["text"]["format"]["type"], "json_schema");
    assert_eq!(json_schema["text"]["format"]["name"], "answer");
    assert_eq!(json_schema["text"]["format"]["strict"], true);
}

#[test]
fn port_chat_to_responses_empty_images_and_files_are_skipped() {
    let out = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        chat_request(json!([{
            "role": "user",
            "content": [
                {"type": "text", "text": "Describe this"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,"}},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,   "}},
                {"type": "file", "file": {"filename": "empty.pdf"}}
            ]
        }])),
    );
    let parts = out["input"][0]["content"].as_array().unwrap();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0]["type"], "input_text");
    assert_eq!(parts[0]["text"], "Describe this");
}

#[test]
fn port_chat_to_responses_file_data_and_id() {
    let data = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        chat_request(json!([{
            "role": "user",
            "content": [
                {"type": "text", "text": "Summarize"},
                {"type": "file", "file": {"filename": "document.pdf", "file_data": "data:application/pdf;base64,JVBERi0xLjQ="}}
            ]
        }])),
    );
    assert_eq!(data["input"][0]["content"][1]["type"], "input_file");
    assert_eq!(data["input"][0]["content"][1]["filename"], "document.pdf");
    assert_eq!(
        data["input"][0]["content"][1]["file_data"],
        "data:application/pdf;base64,JVBERi0xLjQ="
    );

    let id = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        chat_request(
            json!([{"role": "user", "content": [{"type": "file", "file": {"file_id": "file-abc123"}}]}]),
        ),
    );
    assert_eq!(id["input"][0]["content"][0]["type"], "input_file");
    assert_eq!(id["input"][0]["content"][0]["file_id"], "file-abc123");
}

#[test]
fn port_chat_to_responses_empty_content_is_empty_string() {
    let cases = [
        Value::Null,
        json!([]),
        json!([{"type": "text", "text": ""}]),
        json!([{"type": "image_url", "image_url": {"url": "data:image/png;base64,"}}]),
    ];
    for content in cases {
        let out = request(
            ProtocolId::OPENAI_CHAT,
            ProtocolId::OPENAI_RESPONSES,
            chat_request(json!([{"role": "user", "content": content}])),
        );
        assert_ne!(out["input"][0]["content"], Value::Null);
        assert_eq!(out["input"][0]["content"], "");
    }
}

#[test]
fn port_chat_to_responses_assistant_array_and_thinking_tags() {
    let out = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [{
                "role": "assistant",
                "content": [{"type": "text", "text": "before"}, {"type": "text", "text": "after"}]
            }]
        }),
    );
    assert_eq!(out["input"][0]["type"], "message");
    assert_eq!(out["input"][0]["role"], "assistant");

    let thinking = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [{"role": "assistant", "content": "<thinking>plan</thinking>\nanswer"}]
        }),
    );
    assert!(thinking["input"][0]["content"].to_string().contains("plan"));
}

#[test]
fn port_chat_to_responses_temperature_for_reasoning_models() {
    let reasoning = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({"model": "gpt-5.1", "temperature": 0.3, "messages": [{"role": "user", "content": "Hi"}]}),
    );
    assert!(reasoning.get("temperature").is_none());

    let plain = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({"model": "gpt-4o", "temperature": 0.3, "messages": [{"role": "user", "content": "Hi"}]}),
    );
    assert_eq!(plain["temperature"], 0.3);
}

#[test]
fn port_chat_to_responses_system_array_and_legacy_functions() {
    let array = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "system", "content": [{"type": "text", "text": "You are careful."}]},
                {"role": "user", "content": [{"type": "text", "text": "Describe"}, {"type": "image_url", "image_url": {"url": "data:image/png;base64,abc123"}}]}
            ]
        }),
    );
    assert_eq!(array["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(array["input"][0]["content"][0]["text"], "You are careful.");
    assert_eq!(array["input"][1]["content"][1]["type"], "input_image");

    let legacy = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [{"role": "user", "content": "Hi"}],
            "functions": [{"name": "get_weather", "description": "Get weather", "parameters": {"type": "object"}}],
            "function_call": {"name": "get_weather"}
        }),
    );
    assert_eq!(legacy["tools"][0]["type"], "function");
    assert_eq!(legacy["tools"][0]["name"], "get_weather");
    assert_eq!(legacy["tool_choice"]["type"], "function");
    assert_eq!(legacy["tool_choice"]["name"], "get_weather");
    assert!(legacy["tool_choice"].get("function").is_none());
}

#[test]
fn port_chat_to_responses_service_tier_and_parallel_tools() {
    let out = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "service_tier": "priority",
            "parallel_tool_calls": true,
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [{"type": "function", "function": {"name": "a", "parameters": {"type": "object"}}}, {"type": "function", "function": {"name": "b", "parameters": {"type": "object"}}}]
        }),
    );
    assert_eq!(out["service_tier"], "priority");
    assert_eq!(out["parallel_tool_calls"], true);
    assert_eq!(out["tools"].as_array().unwrap().len(), 2);
}

#[test]
fn port_chat_to_responses_tool_array_content_folds_text() {
    let out = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-4o",
            "messages": [
                {"role": "user", "content": "Use the tool"},
                {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "inspect_image", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "call_1", "content": [
                    {"type": "text", "text": "image width: 100"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,ignored"}},
                    {"type": "text", "text": "; image height: 200"}
                ]}
            ]
        }),
    );
    assert_eq!(out["input"][2]["type"], "function_call_output");
    assert_eq!(out["input"][2]["call_id"], "call_1");
    assert_eq!(
        out["input"][2]["output"],
        "image width: 100; image height: 200"
    );
}

#[test]
fn port_responses_to_chat_basic_text_tool_reasoning_and_finish() {
    let basic = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_123",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "Hello, world!"}]}],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
        }),
    );
    assert_eq!(basic["object"], "chat.completion");
    assert_eq!(basic["model"], "gpt-4o");
    assert_eq!(basic["choices"][0]["finish_reason"], "stop");
    assert_eq!(basic["choices"][0]["message"]["content"], "Hello, world!");
    assert_eq!(basic["usage"]["prompt_tokens"], 10);
    assert_eq!(basic["usage"]["completion_tokens"], 5);

    let tool = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_456",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{"type": "function_call", "call_id": "call_xyz", "name": "get_weather", "arguments": "{\"city\":\"NYC\"}"}]
        }),
    );
    assert_eq!(tool["choices"][0]["finish_reason"], "tool_calls");
    assert_eq!(
        tool["choices"][0]["message"]["tool_calls"][0]["id"],
        "call_xyz"
    );
    assert_eq!(
        tool["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
    assert_eq!(
        tool["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        "{\"city\":\"NYC\"}"
    );

    let reasoning = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_789",
            "status": "completed",
            "model": "gpt-4o",
            "output": [
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "I thought about it."}]},
                {"type": "message", "content": [{"type": "output_text", "text": "The answer is 42."}]}
            ]
        }),
    );
    assert_eq!(
        reasoning["choices"][0]["message"]["content"],
        "The answer is 42."
    );
    assert_eq!(
        reasoning["choices"][0]["message"]["reasoning_content"],
        "I thought about it."
    );

    let incomplete = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_inc",
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "model": "gpt-4o",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "partial..."}]}]
        }),
    );
    assert_eq!(incomplete["choices"][0]["finish_reason"], "length");
}

#[test]
fn port_responses_to_chat_token_details_and_web_search() {
    let details = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_full",
            "status": "completed",
            "model": "gpt-5.5",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "x"}]}],
            "usage": {
                "input_tokens": 100, "output_tokens": 50, "total_tokens": 150,
                "input_tokens_details": {"cached_tokens": 60, "audio_tokens": 4},
                "output_tokens_details": {"reasoning_tokens": 30, "audio_tokens": 2, "accepted_prediction_tokens": 10, "rejected_prediction_tokens": 3}
            }
        }),
    );
    assert_eq!(
        details["usage"]["prompt_tokens_details"]["cached_tokens"],
        60
    );
    assert_eq!(details["usage"]["prompt_tokens_details"]["audio_tokens"], 4);
    assert_eq!(
        details["usage"]["completion_tokens_details"]["reasoning_tokens"],
        30
    );
    assert_eq!(
        details["usage"]["completion_tokens_details"]["audio_tokens"],
        2
    );
    assert_eq!(
        details["usage"]["completion_tokens_details"]["accepted_prediction_tokens"],
        10
    );
    assert_eq!(
        details["usage"]["completion_tokens_details"]["rejected_prediction_tokens"],
        3
    );

    let zero = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_no_reasoning",
            "status": "completed",
            "model": "gpt-4o",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "hi"}]}],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15, "output_tokens_details": {"reasoning_tokens": 0}}
        }),
    );
    assert!(zero["usage"].get("completion_tokens_details").is_none());

    let web = response(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_ws",
            "status": "completed",
            "model": "gpt-4o",
            "output": [
                {"type": "web_search_call", "action": {"type": "search", "query": "test"}},
                {"type": "message", "content": [{"type": "output_text", "text": "search results"}]}
            ]
        }),
    );
    assert_eq!(web["choices"][0]["finish_reason"], "stop");
    assert_eq!(web["choices"][0]["message"]["content"], "search results");
}

#[test]
fn port_anthropic_to_responses_basic_and_system() {
    let basic = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-5.2",
            "max_tokens": 1024,
            "stream": true,
            "messages": [{"role": "user", "content": "Hello"}]
        }),
    );
    assert_eq!(basic["model"], "gpt-5.2");
    assert_eq!(basic["stream"], true);
    assert_eq!(basic["max_output_tokens"], 1024);
    assert_eq!(basic["store"], false);
    assert_eq!(basic["input"][0]["type"], "message");
    assert_eq!(basic["input"][0]["role"], "user");
    assert_eq!(basic["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(basic["input"][0]["content"][0]["text"], "Hello");

    let system = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-5.2",
            "max_tokens": 100,
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "Hi"}]
        }),
    );
    assert_eq!(system["input"][0]["role"], "developer");
    assert_eq!(system["input"][0]["content"][0]["text"], "You are helpful.");

    let system_array = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-5.2",
            "max_tokens": 100,
            "system": [{"type": "text", "text": "Part 1"}, {"type": "text", "text": "Part 2"}],
            "messages": [{"role": "user", "content": "Hi"}]
        }),
    );
    assert_eq!(system_array["input"][0]["content"][0]["text"], "Part 1");
    assert_eq!(system_array["input"][0]["content"][1]["text"], "Part 2");

    let billing = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-5.2",
            "max_tokens": 100,
            "system": [{"type": "text", "text": "x-anthropic-billing-header: cc_version=1;"}, {"type": "text", "text": "Project prompt"}],
            "messages": [{"role": "user", "content": "Hi"}]
        }),
    );
    assert_eq!(billing["input"][0]["content"].as_array().unwrap().len(), 1);
    assert_eq!(billing["input"][0]["content"][0]["text"], "Project prompt");
}

#[test]
fn port_anthropic_to_responses_tool_and_thinking() {
    let tool = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-5.2",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "What is the weather?"},
                {"role": "assistant", "content": [{"type": "text", "text": "Let me check."}, {"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "NYC"}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "call_1", "content": "Sunny, 72°F"}]}
            ],
            "tools": [{"name": "get_weather", "description": "Get weather", "input_schema": {"type": "object", "properties": {"city": {"type": "string"}}}}]
        }),
    );
    assert_eq!(tool["tools"][0]["type"], "function");
    assert_eq!(tool["tools"][0]["name"], "get_weather");
    assert_eq!(tool["tools"][0]["strict"], false);
    assert_eq!(tool["input"].as_array().unwrap().len(), 4);
    assert_eq!(tool["input"][2]["type"], "function_call");
    assert_eq!(tool["input"][2]["call_id"], "call_1");
    assert_eq!(tool["input"][3]["type"], "function_call_output");
    assert_eq!(tool["input"][3]["output"], "Sunny, 72°F");

    let no_signature = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "gpt-5.2",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": [{"type": "thinking", "thinking": "deep thought"}, {"type": "text", "text": "Hi!"}]},
                {"role": "user", "content": "More"}
            ]
        }),
    );
    assert_eq!(
        no_signature["input"][1]["content"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(no_signature["input"][1]["content"][0]["text"], "Hi!");

    let signature = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "model": "grok-4.5",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": [{"type": "thinking", "thinking": "plan", "signature": "enc-rs-1"}, {"type": "text", "text": "Hi!"}, {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "ok"}]}
            ]
        }),
    );
    assert_eq!(signature["input"][1]["type"], "reasoning");
    assert_eq!(signature["input"][1]["encrypted_content"], "enc-rs-1");
    assert_eq!(signature["input"][3]["type"], "function_call");
}

#[test]
fn port_anthropic_to_responses_max_tokens_floor() {
    let out = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({"model": "gpt-5.2", "max_tokens": 10, "messages": [{"role": "user", "content": "Hi"}]}),
    );
    assert_eq!(out["max_output_tokens"], 128);
}

#[test]
fn port_responses_to_anthropic_text_and_cached_usage() {
    let text = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_123",
            "model": "gpt-5.2",
            "status": "completed",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "Hello there!"}]}],
            "usage": {"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}
        }),
    );
    assert_eq!(text["id"], "resp_123");
    assert_eq!(text["model"], "gpt-5.2");
    assert_eq!(text["stop_reason"], "end_turn");
    assert_eq!(text["content"][0]["type"], "text");
    assert_eq!(text["content"][0]["text"], "Hello there!");

    let cached = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_cached",
            "model": "gpt-5.2",
            "status": "completed",
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "Cached response"}]}],
            "usage": {"input_tokens": 54006, "output_tokens": 123, "total_tokens": 54129, "input_tokens_details": {"cached_tokens": 1000, "cache_write_tokens": 200}}
        }),
    );
    assert_eq!(cached["usage"]["input_tokens"], 52806);
    assert_eq!(cached["usage"]["cache_read_input_tokens"], 1000);
    assert_eq!(cached["usage"]["cache_creation_input_tokens"], 200);
}

#[test]
fn port_anthropic_to_chat_basic_system_and_tools() {
    let basic = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({"model": "claude-sonnet-4-20250514", "max_tokens": 1024, "messages": [{"role": "user", "content": "hello"}]}),
    );
    assert_eq!(basic["model"], "claude-sonnet-4-20250514");
    assert_eq!(basic["messages"][0]["role"], "user");
    assert_eq!(basic["messages"][0]["content"], "hello");
    assert_eq!(basic["max_completion_tokens"], 1024);

    let system = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({"model": "claude", "max_tokens": 100, "system": "You are helpful", "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert_eq!(system["messages"][0]["role"], "system");
    assert_eq!(system["messages"][0]["content"], "You are helpful");

    let tool = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "claude",
            "max_tokens": 100,
            "messages": [
                {"role": "user", "content": "check weather"},
                {"role": "assistant", "content": [{"type": "text", "text": "Let me check."}, {"type": "tool_use", "id": "toolu_1", "name": "get_weather", "input": {"city": "SF"}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "sunny, 72F"}]}
            ]
        }),
    );
    let messages = tool["messages"].as_array().unwrap();
    let assistant = messages
        .iter()
        .find(|m| m["role"] == "assistant" && m["tool_calls"].is_array())
        .unwrap();
    assert_eq!(assistant["tool_calls"][0]["id"], "toolu_1");
    assert_eq!(
        assistant["tool_calls"][0]["function"]["name"],
        "get_weather"
    );
    assert_eq!(
        assistant["tool_calls"][0]["function"]["arguments"],
        "{\"city\":\"SF\"}"
    );
    let tool_msg = messages.iter().find(|m| m["role"] == "tool").unwrap();
    assert_eq!(tool_msg["tool_call_id"], "toolu_1");
    assert_eq!(tool_msg["content"], "sunny, 72F");
}

#[test]
fn port_anthropic_to_chat_thinking_and_tool_choice() {
    let thinking = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "claude",
            "max_tokens": 100,
            "messages": [{"role": "assistant", "content": [{"type": "thinking", "thinking": "secret thoughts"}, {"type": "text", "text": "answer"}]}]
        }),
    );
    assert_eq!(thinking["messages"][0]["content"], "answer");
    assert!(thinking["messages"][0].get("reasoning_content").is_none());

    let disabled = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({"model": "gpt-5.6-luna", "max_tokens": 1024, "thinking": {"type": "disabled"}, "output_config": {"effort": "max"}, "messages": [{"role": "user", "content": "Hello"}]}),
    );
    assert_eq!(disabled["reasoning_effort"], "none");

    for (choice, expected) in [
        (json!({"type": "auto"}), json!("auto")),
        (json!({"type": "any"}), json!("required")),
    ] {
        let out = request(
            ProtocolId::ANTHROPIC_MESSAGES,
            ProtocolId::OPENAI_CHAT,
            json!({
                "model": "claude",
                "max_tokens": 100,
                "tools": [{"name": "get_weather", "input_schema": {"type": "object", "properties": {}}}],
                "tool_choice": choice,
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );
        assert_eq!(out["tool_choice"], expected);
    }

    let specific = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "claude",
            "max_tokens": 100,
            "tools": [{"name": "get_weather", "input_schema": {"type": "object", "properties": {}}}],
            "tool_choice": {"type": "tool", "name": "get_weather"},
            "messages": [{"role": "user", "content": "hi"}]
        }),
    );
    assert_eq!(specific["tool_choice"]["type"], "function");
    assert_eq!(specific["tool_choice"]["function"]["name"], "get_weather");
}

#[test]
fn port_anthropic_to_chat_reasoning_effort_and_server_tool() {
    let reasoning = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({"model": "claude", "max_tokens": 100, "output_config": {"effort": "high"}, "messages": [{"role": "user", "content": "hi"}]}),
    );
    assert_eq!(reasoning["reasoning_effort"], "high");

    let server_tool = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "claude",
            "max_tokens": 100,
            "tools": [{"type": "web_search_20250305", "name": "web_search"}],
            "messages": [{"role": "user", "content": "hi"}]
        }),
    );
    assert!(server_tool.get("tools").is_none());
}

#[test]
fn port_chat_to_anthropic_response_text_tool_and_finish() {
    let text = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "id": "chatcmpl_1",
            "object": "chat.completion",
            "created": 1,
            "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hello"}, "finish_reason": "stop"}]
        }),
    );
    assert_eq!(text["content"][0]["type"], "text");
    assert_eq!(text["content"][0]["text"], "Hello");
    assert_eq!(text["stop_reason"], "end_turn");

    let tool = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "id": "chatcmpl_2",
            "object": "chat.completion",
            "created": 1,
            "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{}"}}]}, "finish_reason": "tool_calls"}]
        }),
    );
    assert_eq!(tool["content"][0]["type"], "tool_use");
    assert_eq!(tool["content"][0]["id"], "call_1");
    assert_eq!(tool["content"][0]["name"], "f");

    let length = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "id": "chatcmpl_3",
            "object": "chat.completion",
            "created": 1,
            "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "partial"}, "finish_reason": "length"}]
        }),
    );
    assert_eq!(length["stop_reason"], "max_tokens");
}

#[test]
fn port_chat_chunk_to_anthropic_event_sequences() {
    let text = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &[
            json!({"choices": [{"index": 0, "delta": {"role": "assistant", "content": "hello"}}]}),
            json!({"choices": [{"index": 0, "delta": {"content": " world"}}]}),
            json!({"choices": [{"index": 0, "delta": {"content": ""}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 5, "completion_tokens": 2, "total_tokens": 7}}),
        ],
    );
    let types: Vec<&str> = text.iter().filter_map(|e| e["type"].as_str()).collect();
    assert_eq!(
        types,
        vec![
            "message_start",
            "content_block_start",
            "content_block_delta",
            "content_block_delta",
            "content_block_stop",
            "message_delta",
            "message_stop"
        ]
    );

    let reasoning = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &[
            json!({"choices": [{"index": 0, "delta": {"reasoning_content": "thinking..."}}]}),
            json!({"choices": [{"index": 0, "delta": {"content": "answer"}}]}),
            json!({"choices": [{"index": 0, "delta": {"content": ""}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}}),
        ],
    );
    let block_types: Vec<&str> = reasoning
        .iter()
        .filter(|e| e["type"] == "content_block_start")
        .filter_map(|e| e["content_block"]["type"].as_str())
        .collect();
    assert_eq!(block_types, vec!["thinking", "text"]);
}

#[test]
fn port_chat_tool_chunk_aggregation_emits_argument_fragments() {
    let events = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        &[
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": ""}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{\"city\":"}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "\"SF\"}"}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}}),
        ],
    );
    let partials: Vec<&str> = events
        .iter()
        .filter_map(|e| e["delta"]["partial_json"].as_str())
        .collect();
    assert_eq!(partials, vec!["{\"city\":", "\"SF\"}"]);
    let stop = events
        .iter()
        .find(|e| e["type"] == "message_delta")
        .unwrap();
    assert_eq!(stop["delta"]["stop_reason"], "tool_use");
}

#[test]
fn port_responses_to_chat_stream_interleaved_tools_and_usage() {
    let events = stream(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::OPENAI_RESPONSES,
        &[
            json!({"type": "response.created", "response": {"id": "resp_stream"}}),
            json!({"type": "response.output_item.added", "output_index": 1, "item": {"type": "function_call", "call_id": "call_1", "name": "get_weather"}}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "{\"city\":"}),
            json!({"type": "response.output_item.added", "output_index": 2, "item": {"type": "function_call", "call_id": "call_2", "name": "get_time"}}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 2, "delta": "{\"tz\":\"UTC\"}"}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 1, "delta": "\"Tokyo\"}"}),
            json!({"type": "response.completed", "response": {"status": "completed", "usage": {"input_tokens": 50, "output_tokens": 20, "total_tokens": 70, "input_tokens_details": {"cached_tokens": 30}}}}),
        ],
    );
    let calls: Vec<Value> = events
        .iter()
        .filter_map(|e| e["choices"][0]["delta"]["tool_calls"].as_array())
        .flatten()
        .cloned()
        .collect();
    assert!(
        calls
            .iter()
            .any(|call| call["index"] == 0 && call["id"] == "call_1")
    );
    assert!(
        calls
            .iter()
            .any(|call| call["index"] == 1 && call["id"] == "call_2")
    );
    let finish = events
        .iter()
        .find(|e| e["choices"][0]["finish_reason"].is_string())
        .unwrap();
    assert_eq!(finish["choices"][0]["finish_reason"], "tool_calls");
    let usage = events.iter().find(|e| e["usage"].is_object()).unwrap();
    assert_eq!(usage["usage"]["prompt_tokens"], 50);
    assert_eq!(usage["usage"]["completion_tokens"], 20);
    assert_eq!(usage["usage"]["prompt_tokens_details"]["cached_tokens"], 30);
}

#[test]
fn port_responses_to_chat_custom_and_namespace_tools() {
    let custom = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "glm-5.2",
            "input": "run dir",
            "tools": [
                {"type": "custom", "name": "exec", "description": "Run JavaScript code"},
                {"type": "function", "name": "wait", "parameters": {"type": "object", "properties": {}}}
            ]
        }),
    );
    assert_eq!(custom["tools"][0]["type"], "function");
    assert_eq!(custom["tools"][0]["function"]["name"], "exec");
    assert_eq!(
        custom["tools"][0]["function"]["parameters"]["properties"]["input"]["type"],
        "string"
    );
    assert_eq!(custom["tools"][1]["function"]["name"], "wait");

    let namespace = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "deepseek-test",
            "input": "run pwd",
            "tools": [
                {"type": "custom", "name": "exec", "description": "Runs a command"},
                {"type": "namespace", "name": "functions", "tools": [
                    {"type": "function", "name": "wait", "parameters": {"type": "object"}}
                ]}
            ]
        }),
    );
    let names: Vec<&str> = namespace["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["exec", "functions__wait"]);
}

#[test]
fn port_responses_to_chat_additional_tools_and_tool_choice_drop() {
    let additional = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "gpt-test",
            "input": [
                {"type": "additional_tools", "role": "developer", "tools": [
                    {"type": "custom", "name": "exec", "description": "Run PowerShell", "format": {"type": "text"}},
                    {"type": "function", "name": "wait", "parameters": {"type": "object", "properties": {}}},
                    {"type": "namespace", "name": "collaboration", "tools": [
                        {"type": "function", "name": "send_message", "parameters": {"type": "object", "properties": {}}}
                    ]}
                ]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "run Get-Location"}]}
            ],
            "tool_choice": "auto"
        }),
    );
    let names: Vec<&str> = additional["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["exec", "wait", "collaboration__send_message"]);
    assert_eq!(additional["tool_choice"], "auto");
    assert_eq!(additional["messages"].as_array().unwrap().len(), 1);
    assert_eq!(additional["messages"][0]["role"], "user");

    let drop = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "glm-5.2",
            "input": "hi",
            "tools": [{"type": "web_search"}, {"type": "image_generation"}],
            "tool_choice": "auto"
        }),
    );
    assert!(drop.get("tools").is_none());
    assert!(drop.get("tool_choice").is_none());

    let custom_choice = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "glm-5.2",
            "input": "run dir",
            "tools": [{"type": "custom", "name": "exec"}],
            "tool_choice": {"type": "custom", "name": "exec"}
        }),
    );
    assert_eq!(custom_choice["tool_choice"]["type"], "function");
    assert_eq!(custom_choice["tool_choice"]["function"]["name"], "exec");
}

#[test]
fn port_responses_custom_history_and_namespaced_call() {
    let custom = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "m",
            "input": [
                {"role": "user", "content": "list files"},
                {"type": "custom_tool_call", "call_id": "call_1", "name": "exec", "input": "dir"},
                {"type": "custom_tool_call_output", "call_id": "call_1", "output": "main.go"}
            ]
        }),
    );
    let messages = custom["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(messages[1]["tool_calls"][0]["function"]["name"], "exec");
    assert_eq!(
        messages[1]["tool_calls"][0]["function"]["arguments"],
        "{\"input\":\"dir\"}"
    );
    assert_eq!(messages[2]["role"], "tool");
    assert_eq!(messages[2]["tool_call_id"], "call_1");
    assert_eq!(messages[2]["content"], "main.go");

    let namespaced = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "m",
            "input": [
                {"role": "user", "content": "hi"},
                {"type": "function_call", "call_id": "call_1", "namespace": "functions", "name": "wait", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "ok"}
            ],
            "tools": [{"type": "namespace", "name": "functions", "tools": [{"type": "function", "name": "wait", "parameters": {"type": "object"}}]}]
        }),
    );
    let messages = namespaced["messages"].as_array().unwrap();
    assert_eq!(
        messages[1]["tool_calls"][0]["function"]["name"],
        "functions__wait"
    );
}

#[test]
fn port_responses_tool_search_proxy_and_conflicts() {
    let proxy = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "tool_search"}],
            "tool_choice": {"type": "tool_search"}
        }),
    );
    assert_eq!(proxy["tools"][0]["function"]["name"], "tool_search");
    assert_eq!(proxy["tool_choice"]["function"]["name"], "tool_search");

    let conflict = solidway_transcode::registry::direction(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
    )
    .unwrap()
    .request(
        json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "function", "name": "tool_search", "parameters": {"type": "object"}}, {"type": "tool_search"}]
        })
        .to_string()
        .as_bytes(),
        &plan(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT),
    );
    assert!(conflict.is_err());
}

#[test]
fn port_responses_to_anthropic_invalid_blocks_are_sendable() {
    let reasoning = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "glm-5.2",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "run a shell command"}]},
                {"type": "reasoning", "id": "rs_1", "summary": [], "content": [{"type": "reasoning_text", "text": "let me think"}]}
            ]
        }),
    );
    assert_eq!(reasoning["messages"].as_array().unwrap().len(), 1);
    assert!(!reasoning.to_string().contains("reasoning_text"));
    assert!(!reasoning.to_string().contains("let me think"));

    let unknown = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "glm-5.2",
            "input": [{"type": "web_search_call", "id": "ws_1", "content": [{"type": "web_search_result", "text": "payload"}]}]
        }),
    );
    assert!(
        unknown.get("messages").is_none() || unknown["messages"].as_array().unwrap().is_empty()
    );

    let future = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "glm-5.2",
            "input": [{"type": "some_future_item", "content": [
                {"type": "input_text", "text": "keep me"},
                {"type": "reasoning_text", "text": "drop me"}
            ]}]
        }),
    );
    assert!(future.to_string().contains("keep me"));
    assert!(!future.to_string().contains("drop me"));
}

#[test]
fn port_responses_to_anthropic_files_and_empty_messages() {
    let document = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "glm-5.2",
            "input": [{"type": "message", "role": "user", "content": [
                {"type": "input_text", "text": "read this"},
                {"type": "input_file", "filename": "token.pdf", "file_data": "data:application/pdf;base64,JVBERi0="}
            ]}]
        }),
    );
    let block = &document["messages"][0]["content"][1];
    assert_eq!(block["type"], "document");
    assert_eq!(block["source"]["media_type"], "application/pdf");
    assert_eq!(block["source"]["data"], "JVBERi0=");

    let unknown = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "glm-5.2",
            "input": [{"type": "message", "role": "user", "content": [{"type": "input_file", "file_id": "file_1"}]}]
        }),
    );
    assert!(
        unknown.get("messages").is_none() || unknown["messages"].as_array().unwrap().is_empty()
    );

    let assistant = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "glm-5.2",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "refusal", "refusal": "no"}]}
            ]
        }),
    );
    assert_eq!(assistant["messages"].as_array().unwrap().len(), 1);

    let round = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "glm-5.2",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "run ls"}]},
                {"type": "reasoning", "id": "rs_1", "summary": [], "content": [{"type": "reasoning_text", "text": "plan"}], "encrypted_content": "gAAAA"},
                {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "shell", "arguments": "{\"cmd\":\"ls\"}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "file1"},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "continue"}]}
            ]
        }),
    );
    assert!(round.to_string().contains("\"type\":\"tool_use\""));
    assert!(round.to_string().contains("\"type\":\"tool_result\""));
    assert!(!round.to_string().contains("reasoning_text"));
    assert!(!round.to_string().contains("gAAAA"));
}

#[test]
fn port_responses_to_anthropic_tool_schema_normalization() {
    let custom = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "custom", "name": "exec", "description": "Run code", "format": {"type": "grammar", "syntax": "lark", "definition": "start: /[a-z]+/"}}]
        }),
    );
    assert_eq!(custom["tools"][0]["name"], "exec");
    assert_eq!(custom["tools"][0]["input_schema"]["type"], "object");

    let union = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "m",
            "input": "hi",
            "tools": [{"type": "function", "name": "f", "parameters": {
                "anyOf": [
                    {"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]},
                    {"type": "object", "properties": {"b": {"type": "integer"}}, "required": ["b"]}
                ],
                "properties": {"root": {"type": "boolean"}},
                "required": ["root"]
            }}]
        }),
    );
    let schema = &union["tools"][0]["input_schema"];
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["a"]["type"], "string");
    assert_eq!(schema["properties"]["b"]["type"], "integer");
    assert_eq!(schema["properties"]["root"]["type"], "boolean");
    assert_eq!(schema["required"], json!(["root"]));
}

#[test]
fn port_chat_to_anthropic_response_reasoning_fallback_and_empty() {
    let reasoning = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "id": "chatcmpl-3",
            "model": "deepseek-v4-pro",
            "choices": [{"index": 0, "message": {"role": "assistant", "reasoning_content": "I should think about this"}, "finish_reason": "stop"}]
        }),
    );
    assert_eq!(reasoning["content"][0]["type"], "thinking");
    assert_eq!(
        reasoning["content"][0]["thinking"],
        "I should think about this"
    );
    assert_eq!(reasoning["content"][1]["type"], "text");
    assert_eq!(reasoning["content"][1]["text"], "I should think about this");

    let empty = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({"id": "chatcmpl-5", "model": "deepseek-v4-pro", "choices": []}),
    );
    assert_eq!(empty["content"][0]["type"], "text");
    assert_eq!(empty["content"][0]["text"], "");
    assert_eq!(empty["stop_reason"], "end_turn");

    let no_id = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({"model": "deepseek-v4-pro", "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}]}),
    );
    assert!(no_id["id"].as_str().is_some_and(|id| !id.is_empty()));
}

#[test]
fn port_chat_to_anthropic_response_cache_tokens() {
    let out = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "id": "chatcmpl-6",
            "model": "deepseek-v4-pro",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 100, "completion_tokens": 5, "total_tokens": 105, "prompt_tokens_details": {"cached_tokens": 30, "cache_creation_tokens": 10}}
        }),
    );
    assert_eq!(out["usage"]["input_tokens"], 60);
    assert_eq!(out["usage"]["output_tokens"], 5);
    assert_eq!(out["usage"]["cache_read_input_tokens"], 30);
    assert_eq!(out["usage"]["cache_creation_input_tokens"], 10);
}

#[test]
fn port_anthropic_to_chat_tool_choice_undeclared_dropped() {
    let body = json!({
        "model": "deepseek-v4-pro",
        "max_tokens": 100,
        "tools": [
            {"name": "get_weather", "input_schema": {"type": "object"}},
            {"name": "web_search_20250305", "type": "web_search_20250305", "input_schema": {"type": "object"}}
        ],
        "messages": [{"role": "user", "content": "hi"}]
    });
    for choice in [
        json!({"type": "tool", "name": "nonexistent"}),
        json!({"type": "tool", "name": "web_search_20250305"}),
        json!({"type": "mystery"}),
    ] {
        let mut value = body.clone();
        value["tool_choice"] = choice;
        let out = request(
            ProtocolId::ANTHROPIC_MESSAGES,
            ProtocolId::OPENAI_CHAT,
            value,
        );
        assert!(out.get("tool_choice").is_none());
    }

    let mut declared = body;
    declared["tool_choice"] = json!({"type": "tool", "name": "get_weather"});
    let out = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        declared,
    );
    assert_eq!(out["tool_choice"]["type"], "function");
    assert_eq!(out["tool_choice"]["function"]["name"], "get_weather");
}

#[test]
fn port_responses_to_anthropic_tool_stop_reason_and_read_pages() {
    let tool_then_text = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_tool_then_text",
            "model": "gpt-5.5",
            "status": "completed",
            "output": [
                {"type": "function_call", "call_id": "call_todo", "name": "TodoWrite", "arguments": "{\"todos\":[{\"content\":\"review changes\",\"status\":\"in_progress\"}]}"},
                {"type": "message", "content": [{"type": "output_text", "text": "Task list updated."}]}
            ]
        }),
    );
    assert_eq!(tool_then_text["stop_reason"], "tool_use");
    assert_eq!(tool_then_text["content"][0]["type"], "tool_use");
    assert_eq!(tool_then_text["content"][1]["type"], "text");

    let read = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_read",
            "model": "gpt-5.5",
            "status": "completed",
            "output": [{"type": "function_call", "call_id": "call_read", "name": "Read", "arguments": "{\"file_path\":\"/tmp/demo.py\",\"limit\":2000,\"offset\":0,\"pages\":\"\"}"}]
        }),
    );
    assert_eq!(
        read["content"][0]["input"],
        json!({"file_path": "/tmp/demo.py", "limit": 2000, "offset": 0})
    );

    let other = response(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        json!({
            "id": "resp_other",
            "model": "gpt-5.5",
            "status": "completed",
            "output": [{"type": "function_call", "call_id": "call_other", "name": "Search", "arguments": "{\"query\":\"\"}"}]
        }),
    );
    assert_eq!(other["content"][0]["input"], json!({"query": ""}));
}

#[test]
fn port_anthropic_user_array_content_folds_to_string() {
    let out = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "deepseek-v4-pro",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "text", "text": "first"}, {"type": "text", "text": "second"}]}]
        }),
    );
    assert_eq!(out["messages"][0]["content"], "first\n\nsecond");

    let image = request(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "deepseek-v4-pro",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": [{"type": "text", "text": "look"}, {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}}]}]
        }),
    );
    assert!(image["messages"][0]["content"].is_array());
}

#[test]
fn port_responses_agent_messages_become_user() {
    let out = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "glm-5.3",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "<environment_context>cwd</environment_context>"}]},
                {"type": "agent_message", "id": "amsg_1", "author": "/root", "recipient": "/root/alpha_task", "content": [
                    {"type": "input_text", "text": "Message Type: NEW_TASK\nTask name: /root/alpha_task\nSender: /root\nPayload:\n"},
                    {"type": "encrypted_content", "encrypted_content": "Reply with the single word: ALPHA"}
                ]},
                {"type": "reasoning", "summary": [{"type": "summary_text", "text": "just answer"}]},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "ALPHA"}]},
                {"type": "agent_message", "id": "amsg_2", "author": "/root/alpha_task", "recipient": "/root", "content": [
                    {"type": "input_text", "text": "Message Type: FINAL_ANSWER\nTask name: /root\nSender: /root/alpha_task\nPayload:\nALPHA"}
                ]}
            ]
        }),
    );
    let messages = out["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 4);
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(
        messages[1]["content"],
        "Message Type: NEW_TASK\nTask name: /root/alpha_task\nSender: /root\nPayload:\nReply with the single word: ALPHA"
    );
    assert_eq!(messages[2]["role"], "assistant");
    assert_eq!(messages[2]["content"], "ALPHA");
    assert_eq!(messages[3]["role"], "user");
    assert!(messages[3]["content"].as_str().unwrap().contains("ALPHA"));

    let empty = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "glm-5.3",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "agent_message", "author": "/root", "recipient": "/root/a", "content": []},
                {"type": "agent_message", "author": "/root", "recipient": "/root/a"},
                {"type": "agent_message", "author": "/root", "recipient": "/root/a", "content": [{"type": "input_image", "image_url": "data:image/png;base64,AA=="}]}
            ]
        }),
    );
    assert_eq!(empty["messages"].as_array().unwrap().len(), 1);
}

fn anthropic_stream_text(events: &[Value]) -> String {
    events
        .iter()
        .filter(|e| e["type"] == "content_block_delta" && e["delta"]["type"] == "text_delta")
        .filter_map(|e| e["delta"]["text"].as_str())
        .collect()
}

#[test]
fn port_responses_to_anthropic_stream_text_recovery() {
    let done = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &[
            json!({"type": "response.created", "response": {"id": "resp_text_recovery", "model": "gpt-5.2"}}),
            json!({"type": "response.output_item.added", "item": {"type": "message"}}),
            json!({"type": "response.output_text.done", "text": "recovered from done"}),
            json!({"type": "response.completed", "response": {"status": "completed"}}),
        ],
    );
    assert_eq!(anthropic_stream_text(&done), "recovered from done");
    assert_eq!(done.last().unwrap()["type"], "message_stop");

    let terminal = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &[
            json!({"type": "response.created", "response": {"id": "resp_text_recovery"}}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": [{"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "recovered from terminal"}]}]}}),
        ],
    );
    assert_eq!(anthropic_stream_text(&terminal), "recovered from terminal");
}

#[test]
fn port_responses_to_anthropic_stream_text_dedup_and_suffix() {
    let dedup = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &[
            json!({"type": "response.created", "response": {"id": "r"}}),
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": "Hel"}),
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": "lo"}),
            json!({"type": "response.output_text.done", "output_index": 0, "text": "Hello"}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": [{"type": "message", "content": [{"type": "output_text", "text": "Hello"}]}]}}),
        ],
    );
    assert_eq!(anthropic_stream_text(&dedup), "Hello");

    let suffix = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &[
            json!({"type": "response.created", "response": {"id": "r"}}),
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": "Hel"}),
            json!({"type": "response.output_text.done", "output_index": 0, "text": "Hello world"}),
            json!({"type": "response.completed", "response": {"status": "completed", "output": [{"type": "message", "content": [{"type": "output_text", "text": "Hello world"}]}]}}),
        ],
    );
    assert_eq!(anthropic_stream_text(&suffix), "Hello world");

    let divergent = stream(
        ProtocolId::ANTHROPIC_MESSAGES,
        ProtocolId::OPENAI_RESPONSES,
        &[
            json!({"type": "response.created", "response": {"id": "r"}}),
            json!({"type": "response.output_text.delta", "output_index": 0, "delta": "Hello"}),
            json!({"type": "response.output_text.done", "output_index": 0, "text": "Goodbye"}),
            json!({"type": "response.completed", "response": {"status": "completed"}}),
        ],
    );
    assert_eq!(anthropic_stream_text(&divergent), "Hello");
}

fn anthropic_blocks(value: &Value) -> Vec<Value> {
    value["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|message| message["content"].as_array().into_iter().flatten())
        .cloned()
        .collect()
}

#[test]
fn port_anthropic_tool_pairing_normalization() {
    let orphan = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "search the web for X"},
                {"role": "tool", "tool_call_id": "call_orphan", "content": "stale search results"},
                {"role": "assistant", "content": "Here is what I found."},
                {"role": "user", "content": "thanks, now do Y"}
            ]
        }),
    );
    assert!(
        !anthropic_blocks(&orphan)
            .iter()
            .any(|block| block["type"] == "tool_result" && block["tool_use_id"] == "call_orphan")
    );

    let missing = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "search A and B"},
                {"role": "assistant", "content": "searching both", "tool_calls": [
                    {"id": "call_a", "type": "function", "function": {"name": "web_search", "arguments": "{\"q\":\"A\"}"}},
                    {"id": "call_b", "type": "function", "function": {"name": "web_search", "arguments": "{\"q\":\"B\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_a", "content": "result A"}
            ]
        }),
    );
    assert!(
        !anthropic_blocks(&missing)
            .iter()
            .any(|block| block["type"] == "tool_use" && block["id"] == "call_b")
    );
    assert!(
        anthropic_blocks(&missing)
            .iter()
            .any(|block| block["type"] == "tool_use" && block["id"] == "call_a")
    );

    let well_formed = request(
        ProtocolId::OPENAI_CHAT,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "deepseek-v4-pro",
            "messages": [
                {"role": "user", "content": "do A then B"},
                {"role": "assistant", "content": "running A", "tool_calls": [{"id": "call_a", "type": "function", "function": {"name": "exec", "arguments": "{\"cmd\":\"A\"}"}}]},
                {"role": "tool", "tool_call_id": "call_a", "content": "A ok"},
                {"role": "assistant", "content": "A done, running B", "tool_calls": [{"id": "call_b", "type": "function", "function": {"name": "exec", "arguments": "{\"cmd\":\"B\"}"}}]},
                {"role": "tool", "tool_call_id": "call_b", "content": "B ok"},
                {"role": "assistant", "content": "all done"}
            ]
        }),
    );
    let blocks = anthropic_blocks(&well_formed);
    let ids: Vec<&str> = blocks
        .iter()
        .filter(|block| block["type"] == "tool_use")
        .filter_map(|block| block["id"].as_str())
        .collect();
    assert!(ids.contains(&"call_a"));
    assert!(ids.contains(&"call_b"));
}

#[test]
fn port_chat_to_responses_stream_reasoning_only_fallback() {
    let events = stream(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &[
            json!({"choices": [{"index": 0, "delta": {"role": "assistant", "content": null, "reasoning_content": ""}}]}),
            json!({"choices": [{"index": 0, "delta": {"reasoning_content": "thinking before final"}}]}),
            json!({"choices": [{"index": 0, "delta": {"content": ""}, "finish_reason": "length"}], "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}}),
        ],
    );
    let text_deltas: Vec<&str> = events
        .iter()
        .filter(|e| e["type"] == "response.output_text.delta")
        .filter_map(|e| e["delta"].as_str())
        .collect();
    assert_eq!(text_deltas, vec!["thinking before final"]);
    let done = events
        .iter()
        .find(|e| e["type"] == "response.output_text.done")
        .unwrap();
    assert_eq!(done["text"], "thinking before final");
    let completed = events
        .iter()
        .find(|e| e["type"] == "response.completed")
        .unwrap();
    assert_eq!(completed["response"]["status"], "incomplete");
    assert_eq!(
        completed["response"]["incomplete_details"]["reason"],
        "max_output_tokens"
    );
    assert_eq!(completed["response"]["output"].as_array().unwrap().len(), 2);
}

#[test]
fn port_chat_to_responses_stream_reasoning_then_content_no_duplicate() {
    let events = stream(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &[
            json!({"choices": [{"index": 0, "delta": {"reasoning_content": "private plan"}}]}),
            json!({"choices": [{"index": 0, "delta": {"content": "final answer"}}]}),
            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}),
        ],
    );
    let deltas: Vec<&str> = events
        .iter()
        .filter(|e| e["type"] == "response.output_text.delta")
        .filter_map(|e| e["delta"].as_str())
        .collect();
    assert_eq!(deltas, vec!["final answer"]);
    let completed = events
        .iter()
        .find(|e| e["type"] == "response.completed")
        .unwrap();
    assert_eq!(
        completed["response"]["output"][0]["summary"][0]["text"],
        "private plan"
    );
    assert_eq!(
        completed["response"]["output"][1]["content"][0]["text"],
        "final answer"
    );
}

#[test]
fn port_chat_to_responses_stream_tool_lifecycle_and_args() {
    let events = stream(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        &[
            json!({"choices": [{"index": 0, "delta": {"role": "assistant", "reasoning_content": "plan"}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": "call_a", "type": "function", "function": {"name": "exec", "arguments": ""}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{\"cmd\":\"ls\"}"}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}], "usage": {"prompt_tokens": 1, "completion_tokens": 2, "total_tokens": 3}}),
        ],
    );
    let added = events
        .iter()
        .find(|e| e["type"] == "response.output_item.added" && e["item"]["type"] == "function_call")
        .unwrap();
    assert_eq!(added["item"]["call_id"], "call_a");
    assert_eq!(added["item"]["arguments"], "");
    let done = events
        .iter()
        .find(|e| e["type"] == "response.function_call_arguments.done")
        .unwrap();
    assert_eq!(done["arguments"], "{\"cmd\":\"ls\"}");
    let item_done = events
        .iter()
        .find(|e| e["type"] == "response.output_item.done" && e["item"]["type"] == "function_call")
        .unwrap();
    assert_eq!(item_done["item"]["arguments"], "{\"cmd\":\"ls\"}");
    let streamed: String = events
        .iter()
        .filter(|e| e["type"] == "response.function_call_arguments.delta")
        .filter_map(|e| e["delta"].as_str())
        .collect();
    assert_eq!(streamed, "{\"cmd\":\"ls\"}");
}

#[test]
fn port_responses_to_anthropic_pairing_developer_and_parallel() {
    let developer = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "do it"}]},
                {"type": "function_call", "call_id": "call_A", "name": "exec", "arguments": "{}"},
                {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Approved command prefix saved"}]},
                {"type": "function_call_output", "call_id": "call_A", "output": "ok"}
            ]
        }),
    );
    let messages = developer["messages"].as_array().unwrap();
    for (i, message) in messages.iter().enumerate() {
        let uses = message["content"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|block| block["type"] == "tool_use" && block["id"] == "call_A");
        if uses {
            assert_eq!(messages[i + 1]["role"], "user");
            assert!(messages[i + 1]["content"].as_array().unwrap().iter().any(
                |block| block["type"] == "tool_result" && block["tool_use_id"] == "call_A"
            ));
        }
    }

    let parallel = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "features?"}]},
                {"type": "function_call", "call_id": "call_c0", "name": "exec", "arguments": "{}"},
                {"type": "function_call", "call_id": "call_c1", "name": "exec", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_c0", "output": "log"},
                {"type": "function_call_output", "call_id": "call_c1", "output": "tags"}
            ]
        }),
    );
    assert!(
        parallel["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                let blocks = message["content"].as_array().unwrap();
                blocks.iter().any(|b| b["id"] == "call_c0")
                    && blocks.iter().any(|b| b["id"] == "call_c1")
            })
    );

    let missing = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "m",
            "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "q"}]},
                {"type": "function_call", "call_id": "call_A", "name": "exec", "arguments": "{}"},
                {"type": "function_call", "call_id": "call_B", "name": "exec", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_A", "output": "oa"}
            ]
        }),
    );
    assert!(
        !missing["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["content"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["id"] == "call_B"))
    );
}

#[test]
fn port_responses_to_anthropic_function_output_content_array() {
    let out = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::ANTHROPIC_MESSAGES,
        json!({
            "model": "m",
            "input": [
                {"type": "function_call", "call_id": "call_A", "name": "view_image", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "call_A", "output": [
                    {"type": "input_text", "text": "image loaded"},
                    {"type": "input_image", "image_url": "data:image/png;base64,YQ=="}
                ]}
            ]
        }),
    );
    let messages = out["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    let result = &messages[1]["content"][0];
    assert_eq!(result["type"], "tool_result");
    let content = result["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "image loaded");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
    assert_eq!(content[1]["source"]["data"], "YQ==");
}

#[test]
fn port_responses_to_chat_ambiguous_and_duplicate_namespace_names() {
    let direction = solidway_transcode::registry::direction(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
    )
    .unwrap();
    for body in [
        json!({
            "model": "glm-5.2",
            "input": "hi",
            "tools": [
                {"type": "function", "name": "gmail__send"},
                {"type": "namespace", "name": "gmail", "tools": [{"type": "function", "name": "send"}]}
            ]
        }),
        json!({
            "model": "glm-5.2",
            "input": "hi",
            "tools": [
                {"type": "namespace", "name": "a", "tools": [{"type": "function", "name": "b__c"}]},
                {"type": "namespace", "name": "a__b", "tools": [{"type": "function", "name": "c"}]}
            ]
        }),
    ] {
        assert!(
            direction
                .request(
                    body.to_string().as_bytes(),
                    &plan(ProtocolId::OPENAI_RESPONSES, ProtocolId::OPENAI_CHAT)
                )
                .is_err()
        );
    }

    let deduped = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "glm-5.2",
            "input": "hi",
            "tools": [{"type": "namespace", "name": "gmail", "tools": [
                {"type": "function", "name": "send"},
                {"type": "function", "name": "send"}
            ]}]
        }),
    );
    assert_eq!(deduped["tools"].as_array().unwrap().len(), 1);
    assert_eq!(deduped["tools"][0]["function"]["name"], "gmail__send");
}

#[test]
fn port_responses_to_chat_tool_search_discovery_promotion() {
    let out = request(
        ProtocolId::OPENAI_RESPONSES,
        ProtocolId::OPENAI_CHAT,
        json!({
            "model": "m",
            "input": [
                {"type": "tool_search_call", "id": "tsc", "call_id": "call_search", "arguments": {"query": "codex app"}},
                {"type": "tool_search_output", "id": "tso", "call_id": "call_search", "tools": [
                    {"type": "namespace", "name": "codex_app", "tools": [{"type": "function", "name": "load_workspace_dependencies", "parameters": {"type": "object", "properties": {}}}]},
                    {"type": "namespace", "name": "multi_agent_v1", "tools": [
                        {"type": "function", "name": "spawn_agent", "parameters": {"type": "object", "properties": {}}},
                        {"type": "function", "name": "wait_agent", "parameters": {"type": "object", "properties": {}}}
                    ]}
                ]}
            ],
            "tools": [{"type": "tool_search"}]
        }),
    );
    let names: Vec<&str> = out["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"codex_app__load_workspace_dependencies"));
    assert!(names.contains(&"multi_agent_v1__spawn_agent"));
    assert!(names.contains(&"multi_agent_v1__wait_agent"));
    let messages = out["messages"].as_array().unwrap();
    assert!(messages.iter().any(|message| message["role"] == "assistant"
        && message["tool_calls"][0]["function"]["name"] == "tool_search"));
    assert!(messages.iter().any(|message| message["role"] == "tool"));
}
