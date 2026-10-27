//! 会话粘性映射：按 (路由, 模型, 会话标识) 隔离的内存表 + 滑动 TTL。
//!
//! 移植自 develop 的 `affinity.rs`；键中的「池」改为「路由」，值为 provider 凭据 id。

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use tokio::sync::Mutex;

use crate::health::epoch_secs;

/// 一条粘性映射：provider 凭据 id + 过期时刻（epoch 秒）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    provider_id: String,
    expires_at: i64,
}

/// 粘性表键：(路由 id, 模型名, 会话标识)。
type Key = (String, String, String);

/// 会话粘性表。
#[derive(Debug)]
pub struct Affinity {
    ttl: Duration,
    entries: Mutex<HashMap<Key, Entry>>,
}

impl Affinity {
    /// 以配置的 `affinity_ttl` 建立粘性表。
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// 查粘性 provider 并续期（滑动 TTL）；条目已过期则清除并按未命中处理。
    pub async fn lookup(&self, route: &str, model: &str, session: &str) -> Option<String> {
        let now = epoch_secs(SystemTime::now());
        let key = key(route, model, session);
        let mut entries = self.entries.lock().await;
        let entry = entries.get_mut(&key)?;
        if entry.expires_at <= now {
            entries.remove(&key);
            return None;
        }
        entry.expires_at = now.saturating_add(self.ttl.as_secs() as i64);
        Some(entry.provider_id.clone())
    }

    /// 写入 / 更新粘性映射（换号成功与首次绑定共用），并顺带清理过期条目。
    pub async fn remember(&self, route: &str, model: &str, session: &str, provider_id: &str) {
        let now = epoch_secs(SystemTime::now());
        let expires_at = now.saturating_add(self.ttl.as_secs() as i64);
        let mut entries = self.entries.lock().await;
        entries.retain(|_, entry| entry.expires_at > now);
        entries.insert(
            key(route, model, session),
            Entry {
                provider_id: provider_id.to_string(),
                expires_at,
            },
        );
    }

    /// 只读导出未过期的粘性条目（按过期时刻倒序，最多 `limit` 条）。
    ///
    /// 供控制面在收尾时落盘；不修改内存中的映射。
    pub async fn export(&self, limit: usize) -> Vec<crate::config::AffinitySeed> {
        let now = epoch_secs(SystemTime::now());
        let entries = self.entries.lock().await;
        let mut items: Vec<(i64, crate::config::AffinitySeed)> = entries
            .iter()
            .filter(|(_, entry)| entry.expires_at > now)
            .map(|((route, model, session), entry)| {
                (
                    entry.expires_at,
                    crate::config::AffinitySeed {
                        route_id: route.clone(),
                        model: model.clone(),
                        session: session.clone(),
                        provider_id: entry.provider_id.clone(),
                        expires_at: entry.expires_at,
                    },
                )
            })
            .collect();
        items.sort_by_key(|item| std::cmp::Reverse(item.0));
        items.truncate(limit);
        items.into_iter().map(|(_, seed)| seed).collect()
    }

    /// 只保留满足条件的条目（配置下发后清理已删除 / 已归档资源）。
    pub async fn retain<F>(&self, mut keep: F)
    where
        F: FnMut(&str, &str, &str) -> bool,
    {
        let mut entries = self.entries.lock().await;
        entries.retain(|(route, model, _session), entry| keep(route, model, &entry.provider_id));
    }

    /// 启动种子：只在本地无该会话记录时采用（过期条目忽略）。
    ///
    /// 返回采用的条数。
    pub async fn seed(&self, seeds: Vec<crate::config::AffinitySeed>) -> usize {
        let now = epoch_secs(SystemTime::now());
        let mut entries = self.entries.lock().await;
        let mut adopted = 0;
        for seed in seeds {
            if seed.expires_at <= now {
                continue;
            }
            let key = key(&seed.route_id, &seed.model, &seed.session);
            if entries.contains_key(&key) {
                continue;
            }
            entries.insert(
                key,
                Entry {
                    provider_id: seed.provider_id,
                    // 不延长用户原有 TTL：按种子里的过期时刻恢复。
                    expires_at: seed.expires_at,
                },
            );
            adopted += 1;
        }
        adopted
    }

    /// 是否没有任何有效条目（测试与观测用）。
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// 当前有效条目数（测试与观测用）。
    pub async fn len(&self) -> usize {
        let now = epoch_secs(SystemTime::now());
        self.entries
            .lock()
            .await
            .values()
            .filter(|entry| entry.expires_at > now)
            .count()
    }
}

/// 组装粘性键。
fn key(route: &str, model: &str, session: &str) -> Key {
    (route.to_string(), model.to_string(), session.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn miss_returns_none() {
        let affinity = Affinity::new(Duration::from_secs(60));
        assert_eq!(affinity.lookup("r1", "gpt-5", "s1").await, None);
        assert_eq!(affinity.len().await, 0);
    }

    #[tokio::test]
    async fn hit_renews_expiry() {
        let affinity = Affinity::new(Duration::from_secs(60));
        affinity.remember("r1", "gpt-5", "s1", "p-a").await;
        assert_eq!(
            affinity.lookup("r1", "gpt-5", "s1").await.as_deref(),
            Some("p-a")
        );
    }

    #[tokio::test]
    async fn expired_entry_is_cleared() {
        let affinity = Affinity::new(Duration::from_secs(0));
        affinity.remember("r1", "gpt-5", "s1", "p-a").await;
        assert_eq!(affinity.lookup("r1", "gpt-5", "s1").await, None);
        assert_eq!(affinity.len().await, 0);
    }

    #[tokio::test]
    async fn isolated_per_route_and_model() {
        let affinity = Affinity::new(Duration::from_secs(60));
        affinity.remember("r1", "gpt-5", "s1", "p-a").await;
        affinity.remember("r1", "gpt-5-mini", "s1", "p-b").await;
        assert_eq!(affinity.lookup("r2", "gpt-5", "s1").await.as_deref(), None);
        assert_eq!(
            affinity.lookup("r1", "gpt-5", "s1").await.as_deref(),
            Some("p-a")
        );
        assert_eq!(
            affinity.lookup("r1", "gpt-5-mini", "s1").await.as_deref(),
            Some("p-b")
        );
    }

    #[tokio::test]
    async fn export_only_unexpired_and_respects_limit() {
        let affinity = Affinity::new(Duration::from_secs(60));
        affinity.remember("r1", "gpt-5", "s1", "p-a").await;
        affinity.remember("r1", "gpt-5", "s2", "p-b").await;

        let exported = affinity.export(10).await;
        assert_eq!(exported.len(), 2);
        assert!(exported.iter().all(|seed| seed.expires_at > 0));

        assert_eq!(affinity.export(1).await.len(), 1);

        // 过期条目不导出。
        let expired = Affinity::new(Duration::from_secs(0));
        expired.remember("r1", "gpt-5", "s1", "p-a").await;
        assert!(expired.export(10).await.is_empty());
    }

    #[tokio::test]
    async fn seed_adopts_only_when_absent_and_not_expired() {
        let affinity = Affinity::new(Duration::from_secs(60));
        let now = epoch_secs(SystemTime::now());

        let adopted = affinity
            .seed(vec![
                crate::config::AffinitySeed {
                    route_id: "r1".into(),
                    model: "gpt-5".into(),
                    session: "s1".into(),
                    provider_id: "p-seed".into(),
                    expires_at: now + 60,
                },
                // 过期条目忽略。
                crate::config::AffinitySeed {
                    route_id: "r1".into(),
                    model: "gpt-5".into(),
                    session: "s-old".into(),
                    provider_id: "p-old".into(),
                    expires_at: now - 1,
                },
            ])
            .await;
        assert_eq!(adopted, 1);
        assert_eq!(
            affinity.lookup("r1", "gpt-5", "s1").await.as_deref(),
            Some("p-seed")
        );
        assert_eq!(affinity.lookup("r1", "gpt-5", "s-old").await, None);

        // 已有记录不被种子覆盖。
        affinity.remember("r1", "gpt-5", "s1", "p-live").await;
        let adopted = affinity
            .seed(vec![crate::config::AffinitySeed {
                route_id: "r1".into(),
                model: "gpt-5".into(),
                session: "s1".into(),
                provider_id: "p-seed".into(),
                expires_at: now + 120,
            }])
            .await;
        assert_eq!(adopted, 0);
        assert_eq!(
            affinity.lookup("r1", "gpt-5", "s1").await.as_deref(),
            Some("p-live")
        );
    }

    #[tokio::test]
    async fn remember_follows_provider_switch() {
        let affinity = Affinity::new(Duration::from_secs(60));
        affinity.remember("r1", "gpt-5", "s1", "p-a").await;
        affinity.remember("r1", "gpt-5", "s1", "p-b").await;
        assert_eq!(
            affinity.lookup("r1", "gpt-5", "s1").await.as_deref(),
            Some("p-b")
        );
        assert_eq!(affinity.len().await, 1);
    }
}
