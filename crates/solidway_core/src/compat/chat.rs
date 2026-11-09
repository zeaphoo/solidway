//! Chat Completions 请求体的表示层归一化。

use serde_json::Value;

use super::Profile;
use crate::engine::insert_top_level_member;
use crate::jsonbytes::{has_top_level_member, remove_top_level_members, rewrite_message_roles};

/// 一条可应用的改写规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// `developer` 角色 → `system`。
    DeveloperToSystem,
    /// 会话中段的 system/developer → `user`（保留开头连续的系统消息）。
    DowngradeMidSystem,
    /// thinking 参数 → Qwen 顶层 `enable_thinking`。
    ThinkingQwen,
    /// 删除顶层 `store`。
    DropStore,
    /// 删除顶层 `stream_options`。
    DropStreamOptions,
}

impl Rule {
    /// 日志 / 指标使用的稳定标识。
    pub fn id(self) -> &'static str {
        match self {
            Rule::DeveloperToSystem => "developer_to_system",
            Rule::DowngradeMidSystem => "downgrade_mid_system",
            Rule::ThinkingQwen => "thinking_qwen",
            Rule::DropStore => "drop_store",
            Rule::DropStreamOptions => "drop_stream_options",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [
            Rule::DeveloperToSystem,
            Rule::DowngradeMidSystem,
            Rule::ThinkingQwen,
            Rule::DropStore,
            Rule::DropStreamOptions,
        ]
        .into_iter()
        .find(|rule| rule.id() == id)
    }
}

/// 一次归一化的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pub body: Vec<u8>,
    /// 实际生效的规则（按应用顺序）。
    pub rules: Vec<Rule>,
}

impl Rewrite {
    pub fn rule_ids(&self) -> Vec<&'static str> {
        self.rules.iter().map(|rule| rule.id()).collect()
    }
}

/// 请求体里需要 provider 支持的表示层能力（能力路由用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RequestFeatures {
    pub images: bool,
}

/// 轻量扫描请求体里的能力需求；不做 JSON 解析。
pub fn features(body: &[u8]) -> RequestFeatures {
    RequestFeatures {
        images: contains(body, b"\"image_url\"") || contains(body, b"\"input_image\""),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// 按 `profile` 与额外规则（自愈学习到的规则）归一化请求体。
///
/// 没有任何规则命中时返回 `None`，调用方保持原始字节。
pub fn normalize(body: &[u8], profile: &Profile, extra: &[Rule]) -> Option<Rewrite> {
    let mut rules = profile.rules();
    for rule in extra {
        if !rules.contains(rule) {
            rules.push(*rule);
        }
    }
    if rules.is_empty() {
        return None;
    }

    let mut current = body.to_vec();
    let mut applied = Vec::new();
    for rule in rules {
        let changed = match rule {
            Rule::DeveloperToSystem => rewrite_message_roles(&current, "developer", "system"),
            Rule::DowngradeMidSystem => downgrade_mid_conversation_system(&current),
            Rule::ThinkingQwen => thinking_to_qwen(&current),
            Rule::DropStore => remove_top_level_members(&current, &["store"]),
            Rule::DropStreamOptions => remove_top_level_members(&current, &["stream_options"]),
        };
        if let Some(next) = changed {
            current = next;
            applied.push(rule);
        }
    }

    (!applied.is_empty()).then_some(Rewrite {
        body: current,
        rules: applied,
    })
}

/// 把开头连续的系统消息之后出现的 system/developer 降级为 `user`。
///
/// 该规则需要消息顺序信息，因此走一次 JSON DOM；只在 `system_messages = fold`
/// 或上游明确报错时触发，默认路径不解析。
fn downgrade_mid_conversation_system(raw: &[u8]) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(raw).ok()?;
    let messages = value.get_mut("messages")?.as_array_mut()?;
    let mut conversational = false;
    let mut changed = false;
    for message in messages.iter_mut() {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match role.as_str() {
            "system" | "developer" => {
                if conversational {
                    object.insert("role".into(), Value::String("user".into()));
                    changed = true;
                }
            }
            _ => conversational = true,
        }
    }
    changed.then(|| serde_json::to_vec(&value).ok()).flatten()
}

/// `reasoning_effort` / `thinking` → DashScope 顶层布尔 `enable_thinking`。
fn thinking_to_qwen(raw: &[u8]) -> Option<Vec<u8>> {
    if has_top_level_member(raw, "enable_thinking") {
        return None;
    }
    let enabled = thinking_enabled(raw)?;
    let base = remove_top_level_members(raw, &["thinking"]).unwrap_or_else(|| raw.to_vec());
    let member = if enabled {
        r#""enable_thinking":true"#
    } else {
        r#""enable_thinking":false"#
    };
    insert_top_level_member(&base, member)
}

fn thinking_enabled(raw: &[u8]) -> Option<bool> {
    if let Some((start, end)) = crate::engine::top_level_value_span(raw, "thinking") {
        let value: Value = serde_json::from_slice(&raw[start..end]).ok()?;
        if let Some(type_) = value.get("type").and_then(Value::as_str) {
            return Some(matches!(
                type_,
                "enabled" | "adaptive" | "auto" | "on" | "true"
            ));
        }
        if let Some(enabled) = value.as_bool() {
            return Some(enabled);
        }
        return None;
    }
    let (start, end) = crate::engine::top_level_value_span(raw, "reasoning_effort")?;
    // 字符串型 effort 才能判断「关」；其余形状交给上游自行处理。
    let is_string = raw.get(start.wrapping_sub(1)) == Some(&b'"');
    if !is_string {
        return None;
    }
    let effort = &raw[start..end];
    let off = effort.eq_ignore_ascii_case(b"none")
        || effort.eq_ignore_ascii_case(b"off")
        || effort.eq_ignore_ascii_case(b"disabled");
    Some(!off)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compat::{Capability, DeveloperRole, FieldPolicy, SystemMessages, ThinkingShape};

    fn profile() -> Profile {
        Profile::default()
    }

    #[test]
    fn native_profile_returns_none() {
        let body = br#"{"model":"m","messages":[{"role":"developer","content":"x"}]}"#;
        assert!(normalize(body, &profile(), &[]).is_none());
    }

    #[test]
    fn developer_role_is_rewritten_in_place() {
        let body = br#"{"model":"m","messages":[{"role":"developer","content":"x"},{"role":"user","content":"hi"}]}"#;
        let emulated = Profile {
            developer_role: DeveloperRole::Emulated,
            ..Profile::NATIVE
        };
        let rewrite = normalize(body, &emulated, &[]).expect("rewrite");
        assert_eq!(rewrite.rules, vec![Rule::DeveloperToSystem]);
        assert_eq!(
            rewrite.body,
            br#"{"model":"m","messages":[{"role":"system","content":"x"},{"role":"user","content":"hi"}]}"#
        );
    }

    #[test]
    fn rewrite_is_idempotent() {
        let body = br#"{"model":"m","messages":[{"role":"developer","content":"x"}]}"#;
        let emulated = Profile {
            developer_role: DeveloperRole::Emulated,
            ..Profile::NATIVE
        };
        let once = normalize(body, &emulated, &[]).expect("rewrite");
        assert!(normalize(&once.body, &emulated, &[]).is_none());
    }

    #[test]
    fn mid_conversation_system_is_downgraded_with_fold() {
        let body = br#"{"model":"m","messages":[{"role":"system","content":"a"},{"role":"system","content":"b"},{"role":"user","content":"hi"},{"role":"system","content":"c"},{"role":"assistant","content":"d"},{"role":"developer","content":"e"}]}"#;
        let fold = Profile {
            system_messages: SystemMessages::Fold,
            ..Profile::NATIVE
        };
        let rewrite = normalize(body, &fold, &[]).expect("rewrite");
        let parsed: Value = serde_json::from_slice(&rewrite.body).unwrap();
        let roles: Vec<&str> = parsed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect();
        assert_eq!(
            roles,
            vec!["system", "system", "user", "user", "assistant", "user"]
        );
    }

    #[test]
    fn fold_without_mid_conversation_system_is_a_noop() {
        let body = br#"{"model":"m","messages":[{"role":"system","content":"a"},{"role":"user","content":"hi"}]}"#;
        let fold = Profile {
            system_messages: SystemMessages::Fold,
            ..Profile::NATIVE
        };
        assert!(normalize(body, &fold, &[]).is_none());
    }

    #[test]
    fn thinking_reasoning_effort_maps_to_enable_thinking() {
        let qwen = Profile {
            thinking: ThinkingShape::Qwen,
            ..Profile::NATIVE
        };
        let on = normalize(
            br#"{"model":"m","reasoning_effort":"high","messages":[]}"#,
            &qwen,
            &[],
        )
        .expect("rewrite");
        let on_text = String::from_utf8_lossy(&on.body);
        assert!(on_text.contains(r#""enable_thinking":true"#), "{on_text}");
        let off = normalize(
            br#"{"model":"m","reasoning_effort":"none","messages":[]}"#,
            &qwen,
            &[],
        )
        .expect("rewrite");
        assert!(String::from_utf8_lossy(&off.body).contains(r#""enable_thinking":false"#));
    }

    #[test]
    fn thinking_object_maps_to_enable_thinking_and_is_removed() {
        let qwen = Profile {
            thinking: ThinkingShape::Qwen,
            ..Profile::NATIVE
        };
        let rewrite = normalize(
            br#"{"model":"m","thinking":{"type":"disabled","budget_tokens":1},"messages":[]}"#,
            &qwen,
            &[],
        )
        .expect("rewrite");
        let parsed: Value = serde_json::from_slice(&rewrite.body).unwrap();
        assert!(parsed.get("thinking").is_none());
        assert_eq!(parsed["enable_thinking"], Value::Bool(false));
    }

    #[test]
    fn existing_enable_thinking_is_left_alone() {
        let qwen = Profile {
            thinking: ThinkingShape::Qwen,
            ..Profile::NATIVE
        };
        let body =
            br#"{"model":"m","enable_thinking":false,"reasoning_effort":"high","messages":[]}"#;
        assert!(normalize(body, &qwen, &[]).is_none());
    }

    #[test]
    fn drop_fields_removes_only_declared_members() {
        let profile = Profile {
            store: FieldPolicy::Drop,
            stream_options: FieldPolicy::Drop,
            ..Profile::NATIVE
        };
        let body = br#"{"model":"m","stream":true,"store":false,"stream_options":{"include_usage":true},"messages":[]}"#;
        let rewrite = normalize(body, &profile, &[]).expect("rewrite");
        assert_eq!(
            rewrite.body,
            br#"{"model":"m","stream":true,"messages":[]}"#
        );
    }

    #[test]
    fn extra_rules_from_learning_are_applied_once() {
        let body = br#"{"model":"m","messages":[{"role":"developer","content":"x"}]}"#;
        let rewrite =
            normalize(body, &Profile::NATIVE, &[Rule::DeveloperToSystem]).expect("rewrite");
        assert_eq!(rewrite.rules, vec![Rule::DeveloperToSystem]);
        assert!(normalize(&rewrite.body, &Profile::NATIVE, &[Rule::DeveloperToSystem]).is_none());
    }

    #[test]
    fn capabilities_detection() {
        assert!(!features(br#"{"model":"m","messages":[]}"#).images);
        assert!(
            features(br#"{"messages":[{"role":"user","content":[{"type":"image_url"}]}]}"#).images
        );
    }

    #[test]
    fn unsupported_images_skip_the_provider() {
        let profile = Profile {
            images: Capability::Unsupported,
            ..Profile::NATIVE
        };
        assert!(!profile.serves(&RequestFeatures { images: true }));
    }
}
