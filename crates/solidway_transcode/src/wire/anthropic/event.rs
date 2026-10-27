//! Anthropic Messages 流式事件类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::error::AnthropicErrorBody;
use super::response::AnthropicResponse;
use super::types::{AnthropicContentBlock, AnthropicUsage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct AnthropicDelta {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_json: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicEvent {
    #[serde(rename = "type")]
    pub type_: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<AnthropicResponse>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_block: Option<AnthropicContentBlock>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<AnthropicDelta>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<AnthropicUsage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AnthropicErrorBody>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
