//! Chat Completions 流式 chunk 类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::types::{ChatToolCall, ChatUsage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatChunk {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub choices: Vec<ChatChunkChoice>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ChatUsage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatChunkChoice {
    pub index: u32,
    pub delta: ChatDelta,

    /// 非终止 chunk 为 null；不跳过序列化，保持线上 `finish_reason: null`。
    #[serde(default)]
    pub finish_reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<Value>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ChatDelta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ChatToolCall>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
