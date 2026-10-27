//! OpenAI Responses 流式事件类型。
//!
//! Responses 的事件种类很多，但共享同一组字段；这里沿用 sub2api 的做法，
//! 用一个带 `type` 的结构体承载，方向模块按 `type` 分支处理。

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::response::ResponsesResponse;
use super::types::{ResponsesContentPart, ResponsesError, ResponsesOutput, ResponsesUsage};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesEvent {
    #[serde(rename = "type")]
    pub type_: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<ResponsesResponse>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponsesUsage>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ResponsesOutput>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_index: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_index: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_index: Option<u32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<ResponsesContentPart>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_number: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponsesError>,

    #[serde(flatten, default)]
    pub extra: Map<String, Value>,
}
