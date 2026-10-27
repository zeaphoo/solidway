//! OpenAI Responses 错误外壳。

use serde::{Deserialize, Serialize};

use super::types::ResponsesError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponsesErrorEnvelope {
    pub error: ResponsesError,
}
