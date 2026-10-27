//! 方言：私有协议以某个公开 base wire 为骨架，用字段 patch 描述差异。
//!
//! 方言不依赖任何方向模块；组合只发生在 registry。
//! 鉴权、header、账号级 metadata 属于 egress 层，不在方言内处理。

pub(crate) mod claude_code;
pub(crate) mod codex;
pub(crate) mod patch;

use crate::common::frame::{InFrame, OutFrame};
use crate::protocol::{DirectionCaps, ProtocolId};
use crate::report::{Error, Report};

use patch::{FieldOp, apply_ops};

pub(crate) trait Dialect: Send + Sync {
    fn id(&self) -> ProtocolId;
    fn base(&self) -> ProtocolId;

    /// base -> dialect（请求方向）。
    fn to_request_ops(&self) -> &'static [FieldOp];
    /// dialect -> base（请求方向）。
    fn to_base_request_ops(&self) -> &'static [FieldOp];
    /// base -> dialect（响应方向）。
    fn to_response_ops(&self) -> &'static [FieldOp];
    /// dialect -> base（响应方向）。
    fn to_base_response_ops(&self) -> &'static [FieldOp];

    fn caps(&self) -> DirectionCaps {
        DirectionCaps::DEFAULT
    }

    fn encode_request(&self, body: &[u8]) -> Result<(Vec<u8>, Report), Error> {
        apply_ops(body, self.to_request_ops())
    }

    fn decode_request(&self, body: &[u8]) -> Result<(Vec<u8>, Report), Error> {
        apply_ops(body, self.to_base_request_ops())
    }

    fn encode_response(&self, body: &[u8]) -> Result<(Vec<u8>, Report), Error> {
        apply_ops(body, self.to_response_ops())
    }

    fn decode_response(&self, body: &[u8]) -> Result<(Vec<u8>, Report), Error> {
        apply_ops(body, self.to_base_response_ops())
    }

    fn encode_frame(&self, frame: OutFrame) -> Result<(OutFrame, Report), Error> {
        patch_out_frame(frame, self.to_response_ops())
    }

    fn decode_frame(&self, frame: InFrame) -> Result<(InFrame, Report), Error> {
        patch_in_frame(frame, self.to_base_response_ops())
    }
}

static DIALECTS: &[&dyn Dialect] = &[&codex::CODEX, &claude_code::CLAUDE_CODE];

pub(crate) fn all() -> &'static [&'static dyn Dialect] {
    DIALECTS
}

fn patch_in_frame(frame: InFrame, ops: &[FieldOp]) -> Result<(InFrame, Report), Error> {
    match frame {
        InFrame::Sse { event, id, data } => {
            if data == b"[DONE]" || data.is_empty() {
                return Ok((InFrame::Sse { event, id, data }, Report::default()));
            }
            let (data, report) = apply_ops(&data, ops)?;
            Ok((InFrame::Sse { event, id, data }, report))
        }
        other => Ok((other, Report::default())),
    }
}

fn patch_out_frame(frame: OutFrame, ops: &[FieldOp]) -> Result<(OutFrame, Report), Error> {
    match frame {
        OutFrame::Sse { event, id, data } => {
            if data == b"[DONE]" || data.is_empty() {
                return Ok((OutFrame::Sse { event, id, data }, Report::default()));
            }
            let (data, report) = apply_ops(&data, ops)?;
            Ok((OutFrame::Sse { event, id, data }, report))
        }
        other => Ok((other, Report::default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialects_have_public_bases() {
        for dialect in all() {
            assert!(ProtocolId::PUBLIC.contains(&dialect.base()));
            assert_ne!(dialect.id(), dialect.base());
        }
    }
}
