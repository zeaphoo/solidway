//! OpenAI Responses 请求类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::types::{
    ResponsesContent, ResponsesReasoning, ResponsesSummary, ResponsesText, ResponsesTool,
    ResponsesToolOutput,
};

/// `input` 既可以是字符串，也可以是输入项数组。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponsesInput {
    Text(String),
    Items(Vec<ResponsesInputItem>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesInputItem {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<ResponsesContent>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_content: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<ResponsesToolOutput>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Vec<ResponsesSummary>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponsesInput>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ResponsesTool>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponsesReasoning>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<ResponsesText>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
