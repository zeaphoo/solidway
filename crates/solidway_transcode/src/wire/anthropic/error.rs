//! Anthropic Messages 错误外壳。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicError {
    #[serde(rename = "type")]
    pub type_: String,

    pub error: AnthropicErrorBody,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicErrorBody {
    #[serde(rename = "type")]
    pub type_: String,

    pub message: String,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
