//! Claude Code 方言：Anthropic Messages wire + 少量 Claude Code 形状差异。
//!
//! 该模块只做 body 级形状差异；OAuth、beta header、账号级 metadata 属于 egress 层。

use crate::protocol::ProtocolId;

use super::Dialect;
use super::patch::FieldOp;

pub(crate) struct ClaudeCode;

pub(crate) static CLAUDE_CODE: ClaudeCode = ClaudeCode;

static TO_REQUEST: &[FieldOp] = &[
    FieldOp::Default {
        path: "/max_tokens",
        value: "4096",
    },
    FieldOp::Drop {
        path: "/context_management",
        reason: "claude-code does not accept context_management",
    },
    FieldOp::Drop {
        path: "/mcp_servers",
        reason: "claude-code does not accept mcp_servers",
    },
    FieldOp::Drop {
        path: "/container",
        reason: "claude-code does not accept container",
    },
    FieldOp::Default {
        path: "/metadata/user_id",
        value: "\"claude-code\"",
    },
];

static FROM_REQUEST: &[FieldOp] = &[];
static TO_RESPONSE: &[FieldOp] = &[FieldOp::Drop {
    path: "/container",
    reason: "claude-code response has no container field",
}];
static FROM_RESPONSE: &[FieldOp] = &[];

impl Dialect for ClaudeCode {
    fn id(&self) -> ProtocolId {
        ProtocolId::PRIVATE_CLAUDE_CODE
    }

    fn base(&self) -> ProtocolId {
        ProtocolId::ANTHROPIC_MESSAGES
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
}
