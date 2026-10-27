//! OpenAI Chat Completions 线上类型。

mod error;
mod request;
mod response;
mod stream;
mod types;

pub(crate) use error::{ChatError, ChatErrorBody};
pub(crate) use request::{
    ChatFunction, ChatRequest, ChatResponseFormat, ChatStop, ChatTool, ChatToolChoice,
};
pub(crate) use response::{ChatChoice, ChatResponse};
pub(crate) use stream::{ChatChunk, ChatChunkChoice, ChatDelta};
pub(crate) use types::{
    ChatContent, ChatContentPart, ChatFile, ChatFunctionCall, ChatImageUrl, ChatInputAudio,
    ChatMessage, ChatTokenDetails, ChatToolCall, ChatUsage,
};

#[cfg(test)]
mod tests;
