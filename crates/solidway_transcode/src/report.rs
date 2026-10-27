//! 转换报告、策略与错误。

use crate::protocol::ProtocolId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscodePolicy {
    /// 遇到 `Unsupported` 直接失败。
    Strict,
    /// 按能力降级，所有降级 / 丢弃写 warning。
    BestEffort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Source,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningCode {
    Dropped,
    Degraded,
    Synthesized,
    UnknownField,
    UnknownEvent,
    UsageMissing,
    IdSynthesized,
    Truncated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub code: WarningCode,
    pub side: Side,
    pub pointer: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub warnings: Vec<Warning>,
}

impl Report {
    pub fn warn(
        &mut self,
        code: WarningCode,
        side: Side,
        pointer: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.warnings.push(Warning {
            code,
            side,
            pointer: pointer.into(),
            detail: detail.into(),
        });
    }

    pub fn merge(&mut self, other: Report) {
        self.warnings.extend(other.warnings);
    }

    pub fn dropped(&mut self, side: Side, pointer: impl Into<String>, detail: impl Into<String>) {
        self.warn(WarningCode::Dropped, side, pointer, detail);
    }

    pub fn degraded(&mut self, side: Side, pointer: impl Into<String>, detail: impl Into<String>) {
        self.warn(WarningCode::Degraded, side, pointer, detail);
    }

    pub fn unsupported(
        &mut self,
        policy: TranscodePolicy,
        side: Side,
        pointer: impl Into<String>,
        detail: impl Into<String>,
    ) -> Result<(), Error> {
        let pointer = pointer.into();
        let detail = detail.into();
        match policy {
            TranscodePolicy::Strict => Err(Error::Unsupported(format!("{pointer}: {detail}"))),
            TranscodePolicy::BestEffort => {
                self.dropped(side, pointer, detail);
                Ok(())
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid payload: {0}")]
    Invalid(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("no direction from {src} to {dst}")]
    NoDirection { src: ProtocolId, dst: ProtocolId },

    #[error("stream: {0}")]
    Stream(String),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}
