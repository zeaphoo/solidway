//! 自愈学习表：`(provider, model, 格式)` → 曾经必须应用的规则。
//!
//! 只在内存里（`solidway_core` 的纯内存边界），重启或配置下发即失效；
//! 另有 TTL，避免把一次性故障固化成永久改写。

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use super::Rule;

/// 学习条目的存活时间。
const TTL: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    provider: String,
    model: String,
    format: String,
}

impl Key {
    fn new(provider: &str, model: &str, format: &str) -> Self {
        Self {
            provider: provider.to_string(),
            model: model.to_string(),
            format: format.to_string(),
        }
    }
}

/// 学习到的规则集合。
#[derive(Debug, Default)]
pub struct Learned {
    entries: RwLock<HashMap<Key, (Vec<Rule>, Instant)>>,
}

impl Learned {
    pub fn new() -> Self {
        Self::default()
    }

    /// 该组合当前有效的规则（过期的条目顺手清掉）。
    pub fn rules(&self, provider: &str, model: &str, format: &str, now: Instant) -> Vec<Rule> {
        let key = Key::new(provider, model, format);
        let mut entries = self.entries.write().expect("compat lock");
        match entries.get(&key) {
            Some((rules, learned_at)) if now.saturating_duration_since(*learned_at) < TTL => {
                rules.clone()
            }
            Some(_) => {
                entries.remove(&key);
                Vec::new()
            }
            None => Vec::new(),
        }
    }

    /// 记录一次命中并合并规则。
    pub fn remember(
        &self,
        provider: &str,
        model: &str,
        format: &str,
        rules: &[Rule],
        now: Instant,
    ) {
        if rules.is_empty() {
            return;
        }
        let key = Key::new(provider, model, format);
        let mut entries = self.entries.write().expect("compat lock");
        let entry = entries.entry(key).or_insert_with(|| (Vec::new(), now));
        for rule in rules {
            if !entry.0.contains(rule) {
                entry.0.push(*rule);
            }
        }
        entry.1 = now;
    }

    /// 配置下发后清空：规则绑定的是具体上游行为，配置变了就该重新探测。
    pub fn clear(&self) {
        self.entries.write().expect("compat lock").clear();
    }

    pub fn len(&self) -> usize {
        self.entries.read().expect("compat lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_and_merges_rules() {
        let learned = Learned::new();
        let now = Instant::now();
        assert!(learned.rules("p", "m", "chat_completions", now).is_empty());
        learned.remember(
            "p",
            "m",
            "chat_completions",
            &[Rule::DeveloperToSystem],
            now,
        );
        learned.remember("p", "m", "chat_completions", &[Rule::ThinkingQwen], now);
        assert_eq!(
            learned.rules("p", "m", "chat_completions", now),
            vec![Rule::DeveloperToSystem, Rule::ThinkingQwen]
        );
        assert!(
            learned
                .rules("p", "other", "chat_completions", now)
                .is_empty()
        );
    }

    #[test]
    fn entries_expire() {
        let learned = Learned::new();
        let now = Instant::now();
        learned.remember("p", "m", "chat_completions", &[Rule::DropStore], now);
        assert!(
            learned
                .rules("p", "m", "chat_completions", now + TTL)
                .is_empty()
        );
        assert!(learned.is_empty());
    }

    #[test]
    fn clear_drops_everything() {
        let learned = Learned::new();
        let now = Instant::now();
        learned.remember("p", "m", "chat_completions", &[Rule::DropStore], now);
        learned.clear();
        assert!(learned.rules("p", "m", "chat_completions", now).is_empty());
    }
}
