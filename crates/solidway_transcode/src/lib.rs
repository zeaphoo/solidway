//! SolidWay 协议转码库（试验性）。
//!
//! 本 crate 只做协议之间的字节 / 事件转换；不做 IO、路由、鉴权、选号。
//!
//! 当前阶段实现三种公开协议的两两互转（6 个方向）：
//!
//! - `openai.chat`：OpenAI Chat Completions
//! - `openai.responses`：OpenAI Responses
//! - `anthropic.messages`：Anthropic Messages

//! # 示例
//!
//! ```
//! use solidway_transcode::protocol::ProtocolId;
//! use solidway_transcode::registry::{direction, Plan};
//! use solidway_transcode::report::TranscodePolicy;
//!
//! let src = ProtocolId::OPENAI_CHAT;
//! let dst = ProtocolId::ANTHROPIC_MESSAGES;
//! let direction = direction(src, dst).unwrap();
//! let plan = Plan { src, dst, policy: TranscodePolicy::BestEffort };
//! let body = br#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#;
//! let out = direction.request(body, &plan).unwrap();
//! assert!(!out.body.is_empty());
//! ```

pub mod protocol;
pub mod registry;
pub mod report;

pub(crate) mod common;
pub(crate) mod dialect;
pub(crate) mod directions;
pub(crate) mod wire;
