//! Anthropic Messages 请求类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::types::{
    AnthropicContentBlock, AnthropicOutputConfig, AnthropicThinking, AnthropicTool,
};

/// `system` 既可以是字符串，也可以是 text block 数组。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicSystem {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

/// message 的 `content`：字符串或内容块数组。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicMessageContent,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system: Option<AnthropicSystem>,

    pub messages: Vec<AnthropicMessage>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<AnthropicTool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinking>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_config: Option<AnthropicOutputConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_management: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<Value>>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
