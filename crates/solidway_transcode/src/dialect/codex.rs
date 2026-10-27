//! Codex 方言：Responses wire + 少量 Codex 形状差异。
//!
//! 该模块只做 body 级形状差异；OAuth、header、账号/会话 metadata 属于 egress 层。

use crate::protocol::{DirectionCaps, ProtocolId, RequestCaps, Support};

use super::Dialect;
use super::patch::FieldOp;

pub(crate) struct Codex;

pub(crate) static CODEX: Codex = Codex;

static TO_REQUEST: &[FieldOp] = &[
    FieldOp::Set {
        path: "/store",
        value: "false",
    },
    FieldOp::Set {
        path: "/stream",
        value: "true",
    },
    FieldOp::Set {
        path: "/include",
        value: "[\"reasoning.encrypted_content\"]",
    },
    FieldOp::Default {
        path: "/parallel_tool_calls",
        value: "true",
    },
    FieldOp::Drop {
        path: "/metadata",
        reason: "codex does not accept metadata",
    },
    FieldOp::Drop {
        path: "/service_tier",
        reason: "codex does not accept service_tier",
    },
];

static FROM_REQUEST: &[FieldOp] = &[];
static TO_RESPONSE: &[FieldOp] = &[];
static FROM_RESPONSE: &[FieldOp] = &[];

impl Dialect for Codex {
    fn id(&self) -> ProtocolId {
        ProtocolId::PRIVATE_CODEX
    }

    fn base(&self) -> ProtocolId {
        ProtocolId::OPENAI_RESPONSES
    }

    fn to_request_ops(&self) -> &'static [FieldOp] {
        TO_REQUEST
    }

    fn to_base_request_ops(&self) -> &'static [FieldOp] {
        FROM_REQUEST
    }

    fn to_response_ops(&self) -> &'static [FieldOp] {
        TO_RESPONSE
    }

    fn to_base_response_ops(&self) -> &'static [FieldOp] {
        FROM_RESPONSE
    }

    fn caps(&self) -> DirectionCaps {
        DirectionCaps {
            request: RequestCaps {
                custom_tools: Support::Native,
                hosted_tools: Support::Unsupported("codex has no hosted tools"),
                ..RequestCaps::DEFAULT
            },
            ..DirectionCaps::DEFAULT
        }
    }
}
