//! 方向模块。每个方向自包含，不得引用其他方向。

pub(crate) mod anthropic_to_chat;
pub(crate) mod anthropic_to_responses;
pub(crate) mod chat_to_anthropic;
pub(crate) mod chat_to_responses;
pub(crate) mod responses_to_anthropic;
pub(crate) mod responses_to_chat;
