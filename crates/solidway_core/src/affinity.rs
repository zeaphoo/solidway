//! 会话粘性映射：按路由隔离的内存表 + 滑动 TTL。
//!
//! 移植自 develop 的 `affinity.rs`；键中的「池」改为「路由」，值为 provider 凭据 id。
//!
//! 并发约定：
//! - **分片**：每个路由一张独立表、一把独立锁，路由之间互不竞争；全局只有
//!   「路由 id → 分片」这张极短的索引表需要加锁。
//! - **过期清理不在热路径**：`remember` 只做 O(1) 插入，清理按分片惰性触发
//!   （每个分片最多每 `prune_interval` 扫一次），另有 `prune` 供后台任务兜底。
//!   旧实现每个请求都在全局锁内 `retain` 全表，表一大就会把所有转发请求串行化。

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

use tokio::sync::Mutex;

use crate::health::epoch_secs;

/// 一条粘性映射：provider 凭据 id + 过期时刻（epoch 秒）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    provider_id: String,
    expires_at: i64,
}

/// 单路由内的键：(模型名, 会话标识)。
type Key = (String, String);

/// 单路由的粘性表分片。
#[derive(Debug)]
struct Shard {
    entries: Mutex<HashMap<Key, Entry>>,
    /// 下一次惰性清理的时刻（epoch 秒）。
    next_prune_at: AtomicI64,
}

impl Shard {
    fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            next_prune_at: AtomicI64::new(0),
        }
    }
}

/// 会话粘性表。
#[derive(Debug)]
pub struct Affinity {
    ttl: Duration,
    /// 单个分片两次清理之间的最小间隔。
    prune_interval: Duration,
    /// 路由 id → 分片。
    ///
    /// 用 `std::sync::RwLock` 而非 `tokio::Mutex`：临界区只有一次哈希查找 +
    /// `Arc::clone`，从不跨 `.await` 持有（符合「std 锁只用于极短快照」的约定），
    /// 这样数据面就不存在任何全局 await 锁。
    shards: RwLock<HashMap<String, Arc<Shard>>>,
}

impl Affinity {
    /// 以配置的 `affinity_ttl` 建立粘性表。
    pub fn new(ttl: Duration) -> Self {
        // 清理间隔取 TTL 的四分之一（1s ~ 60s）：TTL 越大越没必要频繁扫。
        let prune_interval = (ttl / 4).clamp(Duration::from_secs(1), Duration::from_secs(60));
        Self {
            ttl,
            prune_interval,
            shards: RwLock::new(HashMap::new()),
        }
    }

    /// 取（或建）某路由的分片。
    fn shard(&self, route: &str) -> Arc<Shard> {
        if let Some(shard) = self.index().get(route) {
            return Arc::clone(shard);
        }
        let mut shards = self.shards.write().expect("affinity index lock");
        // 双检：写锁期间可能已有其它任务建好同一路由的分片。
        if let Some(shard) = shards.get(route) {
            return Arc::clone(shard);
        }
        let shard = Arc::new(Shard::new());
        shards.insert(route.to_string(), Arc::clone(&shard));
        shard
    }

    /// 读索引（克隆 Arc 后立即放锁，后续清理不会阻塞其它路由）。
    fn index(&self) -> std::sync::RwLockReadGuard<'_, HashMap<String, Arc<Shard>>> {
        self.shards.read().expect("affinity index lock")
    }

    /// 分片快照：路由 id + Arc，供导出 / 清理遍历。
    fn shard_snapshot(&self) -> Vec<(String, Arc<Shard>)> {
        self.index()
            .iter()
            .map(|(route, shard)| (route.clone(), Arc::clone(shard)))
            .collect()
    }

    /// 查粘性 provider 并续期（滑动 TTL）；条目已过期则清除并按未命中处理。
    pub async fn lookup(&self, route: &str, model: &str, session: &str) -> Option<String> {
        let now = epoch_secs(SystemTime::now());
        // 纯未命中不建分片：`last_used` 首次请求必须先 miss 再 remember。
        let shard = self.index().get(route).cloned()?;
        let mut entries = shard.entries.lock().await;
        let key = key(model, session);
        if entries
            .get(&key)
            .is_some_and(|entry| entry.expires_at <= now)
        {
            entries.remove(&key);
            return None;
        }
        let entry = entries.get_mut(&key)?;
        entry.expires_at = now.saturating_add(self.ttl.as_secs() as i64);
        Some(entry.provider_id.clone())
    }

    /// 写入 / 更新粘性映射（换号成功与首次绑定共用）。
    ///
    /// 热路径上只做插入；过期条目由分片惰性清理（最多每 `prune_interval` 一次）。
    pub async fn remember(&self, route: &str, model: &str, session: &str, provider_id: &str) {
        let now = epoch_secs(SystemTime::now());
        let expires_at = now.saturating_add(self.ttl.as_secs() as i64);
        let shard = self.shard(route);
        let mut entries = shard.entries.lock().await;
        if now >= shard.next_prune_at.load(Ordering::Relaxed) {
            entries.retain(|_, entry| entry.expires_at > now);
            shard.next_prune_at.store(
                now.saturating_add(self.prune_interval.as_secs() as i64),
                Ordering::Relaxed,
            );
        }
        entries.insert(
            key(model, session),
            Entry {
                provider_id: provider_id.to_string(),
                expires_at,
            },
        );
    }

    /// 清理全部分片的过期条目（后台兜底；每个分片独立加锁，不影响转发）。
    pub async fn prune(&self) {
        let now = epoch_secs(SystemTime::now());
        for (_, shard) in self.shard_snapshot() {
            let mut entries = shard.entries.lock().await;
            entries.retain(|_, entry| entry.expires_at > now);
            shard.next_prune_at.store(
                now.saturating_add(self.prune_interval.as_secs() as i64),
                Ordering::Relaxed,
            );
        }
    }

    /// 只读导出未过期的粘性条目（按过期时刻倒序，最多 `limit` 条）。
    ///
    /// 供控制面在收尾时落盘；不修改内存中的映射。
    pub async fn export(&self, limit: usize) -> Vec<crate::config::AffinitySeed> {
        let now = epoch_secs(SystemTime::now());
        let mut items: Vec<(i64, crate::config::AffinitySeed)> = Vec::new();
        for (route_id, shard) in self.shard_snapshot() {
            let entries = shard.entries.lock().await;
            items.extend(
                entries
                    .iter()
                    .filter(|(_, entry)| entry.expires_at > now)
                    .map(|((model, session), entry)| {
                        (
                            entry.expires_at,
                            crate::config::AffinitySeed {
                                route_id: route_id.clone(),
                                model: model.clone(),
                                session: session.clone(),
                                provider_id: entry.provider_id.clone(),
                                expires_at: entry.expires_at,
                            },
                        )
                    }),
            );
        }
        items.sort_by_key(|item| std::cmp::Reverse(item.0));
        items.truncate(limit);
        items.into_iter().map(|(_, seed)| seed).collect()
    }

    /// 只保留满足条件的条目（配置下发后清理已删除 / 已归档资源）。
    pub async fn retain<F>(&self, mut keep: F)
    where
        F: FnMut(&str, &str, &str) -> bool,
    {
        let mut emptied: Vec<(String, Arc<Shard>)> = Vec::new();
        for (route_id, shard) in self.shard_snapshot() {
            let mut entries = shard.entries.lock().await;
            entries.retain(|(model, _session), entry| keep(&route_id, model, &entry.provider_id));
            if entries.is_empty() {
                emptied.push((route_id, Arc::clone(&shard)));
            }
        }
        // 整条路由都被清掉时连分片一起丢；只在「仍然是同一个分片且仍为空」时删，
        // 避免删掉清理期间刚被重新写入的分片。
        if emptied.is_empty() {
            return;
        }
        let mut shards = self.shards.write().expect("affinity index lock");
        for (route_id, shard) in emptied {
            let same = shards
                .get(&route_id)
                .is_some_and(|existing| Arc::ptr_eq(existing, &shard));
            if !same {
                continue;
            }
            let empty = match shard.entries.try_lock() {
                Ok(entries) => entries.is_empty(),
                Err(_) => false,
            };
            if empty {
                shards.remove(&route_id);
            }
        }
    }

    /// 启动种子：只在本地无该会话记录时采用（过期条目忽略）。
    ///
    /// 返回采用的条数。
    pub async fn seed(&self, seeds: Vec<crate::config::AffinitySeed>) -> usize {
        let now = epoch_secs(SystemTime::now());
        let mut by_route: HashMap<String, Vec<crate::config::AffinitySeed>> = HashMap::new();
        for seed in seeds {
            if seed.expires_at <= now {
                continue;
            }
            by_route
                .entry(seed.route_id.clone())
                .or_default()
                .push(seed);
        }
        let mut adopted = 0;
        for (route_id, seeds) in by_route {
            let shard = self.shard(&route_id);
            let mut entries = shard.entries.lock().await;
            for seed in seeds {
                let key = key(&seed.model, &seed.session);
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
        let mut total = 0;
        for (_, shard) in self.shard_snapshot() {
            total += shard
                .entries
                .lock()
                .await
                .values()
                .filter(|entry| entry.expires_at > now)
                .count();
        }
        total
    }

    /// 列举某路由的未过期条目（按过期时刻倒序，最多 `limit` 条）。
    ///
    /// 供管理面展示与会话诊断用（与 [`Self::export`] 的区别：只取单路由）。
    pub async fn list(&self, route: &str, limit: usize) -> Vec<crate::config::AffinitySeed> {
        let now = epoch_secs(SystemTime::now());
        let shard = self.index().get(route).cloned();
        let Some(shard) = shard else {
            return Vec::new();
        };
        let entries = shard.entries.lock().await;
        let mut items: Vec<(i64, crate::config::AffinitySeed)> = entries
            .iter()
            .filter(|(_, entry)| entry.expires_at > now)
            .map(|((model, session), entry)| {
                (
                    entry.expires_at,
                    crate::config::AffinitySeed {
                        route_id: route.to_string(),
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

    /// 删除某路由的粘性条目：给出 `(model, session)` 时只删该组合，否则清空该路由。
    ///
    /// 返回实际删除的条数（含已过期但未清理的条目）。
    pub async fn remove(&self, route: &str, model_and_session: Option<(&str, &str)>) -> usize {
        match model_and_session {
            Some((model, session)) => {
                let shard = self.index().get(route).cloned();
                let Some(shard) = shard else {
                    return 0;
                };
                let removed = shard
                    .entries
                    .lock()
                    .await
                    .remove(&key(model, session))
                    .is_some();
                usize::from(removed)
            }
            // 整条路由清空：在分片锁内 `clear`，与并发 `remember` 的可见性等价于
            // 「清除先于后续写入生效」，不会丢掉清理期间刚写入的条目。
            None => {
                let shard = self.index().get(route).cloned();
                let Some(shard) = shard else {
                    return 0;
                };
                let mut entries = shard.entries.lock().await;
                let removed = entries.len();
                entries.clear();
                removed
            }
        }
    }
}

/// 组装粘性键。
fn key(model: &str, session: &str) -> Key {
    (model.to_string(), session.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_is_scoped_to_route_and_sorted() {
        let affinity = Affinity::new(Duration::from_secs(600));
        affinity.remember("r1", "m1", "s1", "p-a").await;
        affinity.remember("r1", "m2", "s2", "p-b").await;
        affinity.remember("r2", "m1", "s1", "p-c").await;
        let listed = affinity.list("r1", 10).await;
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|item| item.route_id == "r1"));
        assert_eq!(affinity.list("r1", 1).await.len(), 1);
    }

    #[tokio::test]
    async fn remove_by_pair_and_by_route() {
        let affinity = Affinity::new(Duration::from_secs(600));
        affinity.remember("r1", "m1", "s1", "p-a").await;
        affinity.remember("r1", "m2", "s2", "p-b").await;
        affinity.remember("r2", "m1", "s1", "p-c").await;

        assert_eq!(affinity.remove("r1", Some(("m1", "s1"))).await, 1);
        assert_eq!(affinity.len().await, 2);
        assert_eq!(affinity.remove("r1", None).await, 1);
        assert_eq!(affinity.len().await, 1);
        assert_eq!(affinity.remove("nope", None).await, 0);
    }

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

    #[tokio::test]
    async fn prune_drops_expired_entries_per_shard() {
        let affinity = Affinity::new(Duration::from_secs(0));
        affinity.remember("r1", "gpt-5", "s1", "p-a").await;
        affinity.remember("r2", "gpt-5", "s2", "p-b").await;
        affinity.prune().await;
        assert_eq!(affinity.len().await, 0);
        assert_eq!(affinity.export(10).await.len(), 0);
    }

    #[tokio::test]
    async fn retain_drops_shard_of_removed_route() {
        let affinity = Affinity::new(Duration::from_secs(600));
        affinity.remember("r1", "gpt-5", "s1", "p-a").await;
        affinity.remember("r2", "gpt-5", "s2", "p-b").await;
        affinity
            .retain(|route, _model, _provider| route == "r1")
            .await;
        assert_eq!(affinity.len().await, 1);
        assert_eq!(affinity.lookup("r2", "gpt-5", "s2").await, None);
        assert!(!affinity.is_empty().await);
    }
}
