//! 空响应合成：上游返回空 body / null 时保持客户端可用。

use serde_json::{Value, json};

pub(crate) fn chat_response() -> Value {
    json!({
        "id": "chatcmpl_transcoded",
        "object": "chat.completion",
        "created": 0,
        "model": "",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": ""},
            "finish_reason": "stop"
        }]
    })
}

pub(crate) fn anthropic_response() -> Value {
    json!({
        "id": "msg_transcoded",
        "type": "message",
        "role": "assistant",
        "model": "",
        "content": [{"type": "text", "text": ""}],
        "stop_reason": "end_turn"
    })
}

pub(crate) fn responses_response() -> Value {
    json!({
        "id": "resp_transcoded",
        "object": "response",
        "created_at": 1,
        "model": "",
        "status": "completed",
        "output": [{
            "type": "message",
            "id": "msg_transcoded",
            "role": "assistant",
            "status": "completed",
            "content": [{"type": "output_text", "text": ""}]
        }]
    })
}

pub(crate) fn is_empty_body(body: &[u8]) -> bool {
    let trimmed = body.trim_ascii();
    trimmed.is_empty() || trimmed == b"null"
}
