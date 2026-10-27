//! 三种公开协议的线上类型（wire）。
//!
//! 每个子模块只描述自己的协议，不与其他 wire 共享消息 / 内容块类型。

pub(crate) mod anthropic;
pub(crate) mod openai_chat;
pub(crate) mod openai_responses;
