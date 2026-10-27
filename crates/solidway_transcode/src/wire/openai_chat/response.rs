//! Chat Completions 非流式响应类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::types::{ChatMessage, ChatUsage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub choices: Vec<ChatChoice>,

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
pub struct ChatChoice {
    pub index: u32,
    pub message: ChatMessage,
    pub finish_reason: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<Value>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
