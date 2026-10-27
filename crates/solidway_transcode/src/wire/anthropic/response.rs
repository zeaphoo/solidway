//! Anthropic Messages 非流式响应类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::types::{AnthropicContentBlock, AnthropicUsage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicResponse {
    #[serde(default)]
    pub id: String,

    #[serde(rename = "type", default)]
    pub type_: String,

    #[serde(default)]
    pub role: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<AnthropicContentBlock>,

    #[serde(default)]
    pub model: String,

    /// 终止原因在 message_start 阶段是 null；不跳过序列化，保持线上形状。
    #[serde(default)]
    pub stop_reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AnthropicUsage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<Value>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
