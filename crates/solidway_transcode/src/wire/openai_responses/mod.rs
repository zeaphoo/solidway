//! OpenAI Responses 线上类型。

mod error;
mod event;
mod request;
mod response;
mod types;

pub(crate) use error::ResponsesErrorEnvelope;
pub(crate) use event::ResponsesEvent;
pub(crate) use request::{ResponsesInput, ResponsesInputItem, ResponsesRequest};
pub(crate) use response::ResponsesResponse;
pub(crate) use types::{
    ResponsesContent, ResponsesContentPart, ResponsesError, ResponsesImageUrl,
    ResponsesIncompleteDetails, ResponsesInputTokensDetails, ResponsesOutput,
    ResponsesOutputTokensDetails, ResponsesReasoning, ResponsesSummary, ResponsesText,
    ResponsesTool, ResponsesToolOutput, ResponsesUsage,
};

#[cfg(test)]
mod tests;
