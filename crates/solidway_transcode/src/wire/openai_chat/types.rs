//! Chat Completions 请求 / 响应共用的线上类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `content` 既可以是字符串，也可以是内容块数组。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
    Text(String),
    Parts(Vec<ChatContentPart>),
}

/// Chat Completions 的消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ChatContent>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ChatToolCall>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_call: Option<ChatFunctionCall>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

/// 多模态内容块。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatContentPart {
    #[serde(rename = "type")]
    pub type_: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ChatImageUrl>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio: Option<ChatInputAudio>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<ChatFile>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_breakpoint: Option<Value>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatImageUrl {
    pub url: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatInputAudio {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_data: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

/// 工具调用。`index` 只在流式 chunk 中出现。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    #[serde(default)]
    pub function: ChatFunctionCall,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatFunctionCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// 流式 delta 中可能只带 name 不带 arguments。
    #[serde(default)]
    pub arguments: String,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens_details: Option<ChatTokenDetails>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens_details: Option<ChatTokenDetails>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatTokenDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_prediction_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_prediction_tokens: Option<u64>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
