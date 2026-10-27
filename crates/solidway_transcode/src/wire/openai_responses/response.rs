//! OpenAI Responses 非流式响应类型。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::types::{
    ResponsesError, ResponsesIncompleteDetails, ResponsesOutput, ResponsesReasoning, ResponsesText,
    ResponsesTool, ResponsesUsage,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesResponse {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub status: String,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub output: Vec<ResponsesOutput>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponsesUsage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<ResponsesIncompleteDetails>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponsesError>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponsesReasoning>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<ResponsesText>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ResponsesTool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation: Option<String>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
