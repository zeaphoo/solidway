//! 上游兼容适配档（表示层）。
//!
//! `chat_completions` 只是一层宽松族：同名协议不代表同名行为。不同上游在角色集合、
//! thinking 参数形状、必填组合上并不一致，客户端按其自身假设发出的请求会被严格上游拒绝。
//!
//! 本模块把「厂商差异」表达为**数据**（[`Profile`]），由控制面按 provider 预设冻结下发：
//!
//! - `profile == NATIVE`（默认）时数据面逐字节透传，不产生任何改写；
//! - 只改写上游明确不接受的表示层（角色名、参数形状），不改业务内容（正文、工具 schema）；
//! - 规则幂等、定点改写（不整包 re-serialize），其余字节保持原样。

mod chat;
mod learned;
mod signature;

pub use chat::{RequestFeatures, Rewrite, Rule, features, normalize};
pub use learned::Learned;
pub use signature::diagnose;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 指令角色的表示方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeveloperRole {
    /// 上游原生接受 `developer`。
    #[default]
    Native,
    /// 上游只认 `system`：把 `developer` 改写为 `system`。
    Emulated,
}

/// 会话中段指令消息的处理方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SystemMessages {
    /// 上游接受任意位置的 system。
    #[default]
    Native,
    /// 上游只接受开头连续的系统消息：其余降级为 `user`。
    Fold,
}

/// thinking 参数形状。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingShape {
    /// 原样透传（`reasoning_effort` / `thinking` 等）。
    #[default]
    Native,
    /// DashScope / Qwen：顶层布尔 `enable_thinking`。
    Qwen,
}

/// 单个顶层字段的取舍。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldPolicy {
    /// 原样透传。
    #[default]
    Native,
    /// 上游不接受该字段：删除。
    Drop,
}

/// 媒体输入能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    #[default]
    Native,
    /// 上游不接受图片输入：含图片的请求不选该 provider（能力路由）。
    Unsupported,
}

/// 一个 provider 的兼容适配档。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub developer_role: DeveloperRole,
    pub system_messages: SystemMessages,
    pub thinking: ThinkingShape,
    pub store: FieldPolicy,
    pub stream_options: FieldPolicy,
    pub images: Capability,
}

impl Profile {
    /// 全原生：与未引入适配档之前逐字节一致。
    pub const NATIVE: Self = Self {
        developer_role: DeveloperRole::Native,
        system_messages: SystemMessages::Native,
        thinking: ThinkingShape::Native,
        store: FieldPolicy::Native,
        stream_options: FieldPolicy::Native,
        images: Capability::Native,
    };

    /// 从 provider 的 `options.compat` 解析；缺省或非法时按 native 处理（fail-open）。
    pub fn from_options(options: Option<&Value>) -> Self {
        let Some(value) = options.and_then(|options| options.get("compat")) else {
            return Self::NATIVE;
        };
        serde_json::from_value(value.clone()).unwrap_or_else(|error| {
            tracing::warn!(%error, "compat 适配档非法，按 native 处理");
            Self::NATIVE
        })
    }

    pub fn is_native(&self) -> bool {
        *self == Self::NATIVE
    }

    /// 该档需要应用的规则（顺序固定，保证确定性）。
    pub fn rules(&self) -> Vec<Rule> {
        let mut rules = Vec::new();
        if self.developer_role == DeveloperRole::Emulated {
            rules.push(Rule::DeveloperToSystem);
        }
        if self.system_messages == SystemMessages::Fold {
            rules.push(Rule::DowngradeMidSystem);
        }
        if self.thinking == ThinkingShape::Qwen {
            rules.push(Rule::ThinkingQwen);
        }
        if self.store == FieldPolicy::Drop {
            rules.push(Rule::DropStore);
        }
        if self.stream_options == FieldPolicy::Drop {
            rules.push(Rule::DropStreamOptions);
        }
        rules
    }

    /// 取更保守的一方（同路由多 provider 对外声明时使用）。
    pub fn intersect(&mut self, other: &Self) {
        if other.developer_role == DeveloperRole::Emulated {
            self.developer_role = DeveloperRole::Emulated;
        }
        if other.system_messages == SystemMessages::Fold {
            self.system_messages = SystemMessages::Fold;
        }
        if other.thinking == ThinkingShape::Qwen {
            self.thinking = ThinkingShape::Qwen;
        }
        if other.store == FieldPolicy::Drop {
            self.store = FieldPolicy::Drop;
        }
        if other.stream_options == FieldPolicy::Drop {
            self.stream_options = FieldPolicy::Drop;
        }
        if other.images == Capability::Unsupported {
            self.images = Capability::Unsupported;
        }
    }

    /// 网关是否可以为该 provider 补 `stream_options`。
    pub fn allows_stream_options(&self) -> bool {
        self.stream_options == FieldPolicy::Native
    }

    /// 该 provider 是否能承载请求所需的表示层能力（能力路由用）。
    pub fn serves(&self, features: &RequestFeatures) -> bool {
        !(features.images && self.images == Capability::Unsupported)
    }

    /// 把适配档折算进协议方向的能力矩阵（跨协议转码时一并收窄）。
    pub fn narrow_request_caps(&self, caps: &mut solidway_transcode::protocol::RequestCaps) {
        use solidway_transcode::protocol::Support;
        if self.developer_role == DeveloperRole::Emulated {
            caps.developer_role = Support::Emulated;
        }
        if self.system_messages == SystemMessages::Fold {
            caps.system_messages = Support::Emulated;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_native() {
        assert!(Profile::default().is_native());
        assert_eq!(Profile::from_options(None), Profile::NATIVE);
        assert!(Profile::from_options(Some(&serde_json::json!({}))).is_native());
    }

    #[test]
    fn parses_options_compat() {
        let options =
            serde_json::json!({"compat": {"developer_role": "emulated", "thinking": "qwen"}});
        let profile = Profile::from_options(Some(&options));
        assert_eq!(profile.developer_role, DeveloperRole::Emulated);
        assert_eq!(profile.thinking, ThinkingShape::Qwen);
        assert_eq!(profile.store, FieldPolicy::Native);
        assert_eq!(
            profile.rules(),
            vec![Rule::DeveloperToSystem, Rule::ThinkingQwen]
        );
    }

    #[test]
    fn invalid_compat_falls_back_to_native() {
        let options = serde_json::json!({"compat": {"developer_role": "nope"}});
        assert!(Profile::from_options(Some(&options)).is_native());
    }

    #[test]
    fn intersect_takes_the_more_conservative_side() {
        let mut left = Profile::NATIVE;
        let right = Profile {
            developer_role: DeveloperRole::Emulated,
            thinking: ThinkingShape::Qwen,
            images: Capability::Unsupported,
            ..Profile::NATIVE
        };
        left.intersect(&right);
        assert_eq!(left.developer_role, DeveloperRole::Emulated);
        assert_eq!(left.thinking, ThinkingShape::Qwen);
        assert_eq!(left.images, Capability::Unsupported);
    }

    #[test]
    fn capability_routing_skips_image_requests() {
        let profile = Profile {
            images: Capability::Unsupported,
            ..Profile::NATIVE
        };
        assert!(profile.serves(&RequestFeatures { images: false }));
        assert!(!profile.serves(&RequestFeatures { images: true }));
    }

    #[test]
    fn profile_narrows_transcode_caps() {
        let mut caps = solidway_transcode::protocol::RequestCaps::DEFAULT;
        Profile {
            developer_role: DeveloperRole::Emulated,
            ..Profile::NATIVE
        }
        .narrow_request_caps(&mut caps);
        assert_eq!(
            caps.developer_role,
            solidway_transcode::protocol::Support::Emulated
        );
    }
}
