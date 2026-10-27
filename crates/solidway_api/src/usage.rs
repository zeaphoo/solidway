//! 用量采集：事件 → 内存聚合桶（小时 / 天）→ 定期落盘；逐条明细按设置保留。
//!
//! 设计口径（模式 B）：聚合常开；错误/换号明细常记；成功明细按 `detail_mode` 保留。
//! token 只写入聚合桶，不依赖明细保留。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use solidway_core::{AccessEvent, AttemptEvent, CompletionEvent, CompletionOutcome};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::db::routes;
use crate::db::usage::{self, AttemptDelta, BucketDelta, HIST_BUCKETS};
use crate::error::Result;
use crate::settings::{self, DetailMode, UsageSettings};
use crate::state::now_unix;

/// 小时桶保留（内部固定，不入设置）。
const HOUR_RETENTION_DAYS: i64 = 365;
/// 天桶保留（内部固定，不入设置）。
const DAY_RETENTION_DAYS: i64 = 730;

#[derive(Hash, PartialEq, Eq, Clone)]
struct Key {
    granularity: &'static str,
    bucket_ts: i64,
    route_id: String,
    api_key_id: String,
    model: String,
    provider_id: String,
    owner_id: String,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct AttemptKey {
    granularity: &'static str,
    bucket_ts: i64,
    route_id: String,
    provider_id: String,
    model: String,
}

/// 本地时区对齐的桶起点。
fn bucket_start(ts: i64, granularity: &str, offset_minutes: i64) -> i64 {
    let offset = offset_minutes * 60;
    let size = if granularity == "day" { 86_400 } else { 3_600 };
    ts - (ts + offset).rem_euclid(size)
}

fn hist_index(latency_ms: u64) -> usize {
    HIST_BUCKETS
        .iter()
        .position(|upper| latency_ms <= *upper)
        .unwrap_or(HIST_BUCKETS.len() - 1)
}

fn status_class(status: u16) -> (i64, i64) {
    if (400..500).contains(&status) {
        (1, 0)
    } else if status >= 500 {
        (0, 1)
    } else {
        (0, 0)
    }
}

/// 明细缓冲里的一条：`Access` 侧与 `Completion` 侧分开累积（事件顺序无关）。
#[derive(Debug, Default)]
struct PendingDetail {
    insert: Option<crate::db::detail::DetailInsert>,
    completion: Option<crate::db::detail::DetailCompletion>,
}

/// 明细批量落库周期。
pub const DETAIL_FLUSH_INTERVAL: Duration = Duration::from_millis(2_500);

/// 用量采集器。
pub struct UsageCollector {
    pool: SqlitePool,
    /// 明细库（独立文件）；不可用时明细功能降级关闭。
    detail: Option<crate::db::detail::DetailStore>,
    buffer: Mutex<HashMap<Key, BucketDelta>>,
    attempt_buffer: Mutex<HashMap<AttemptKey, AttemptDelta>>,
    detail_buffer: Mutex<HashMap<String, PendingDetail>>,
    settings: Mutex<UsageSettings>,
    /// 路由 → owner 缓存；用量写入时固化为历史归属。
    owners: Mutex<HashMap<String, String>>,
}

impl UsageCollector {
    pub async fn new(pool: SqlitePool, detail: Option<crate::db::detail::DetailStore>) -> Self {
        let settings = settings::load_usage(&pool).await.unwrap_or_default();
        let owners = routes::owner_map(&pool).await.unwrap_or_default();
        Self {
            pool,
            detail,
            buffer: Mutex::new(HashMap::new()),
            attempt_buffer: Mutex::new(HashMap::new()),
            detail_buffer: Mutex::new(HashMap::new()),
            settings: Mutex::new(settings),
            owners: Mutex::new(owners),
        }
    }

    /// 记录一次逻辑请求（连同其尝试列表）。
    pub async fn record(&self, access: &AccessEvent, attempts: &[AttemptEvent]) {
        let settings = self.settings.lock().await.clone();
        if !settings.enabled {
            return;
        }

        let owner_id = self.owner_for(&access.route_id).await;
        let failovers = access.attempts.saturating_sub(1) as i64;
        let mut hist = [0i64; 11];
        if access.ttfb_ms > 0 {
            hist[hist_index(access.ttfb_ms)] = 1;
        }
        let is_error = access.status >= 400;
        let (client_error, server_error) = status_class(access.status);

        {
            let mut buffer = self.buffer.lock().await;
            for granularity in ["hour", "day"] {
                let key = Key {
                    granularity,
                    bucket_ts: bucket_start(
                        access.at,
                        granularity,
                        settings.timezone_offset_minutes,
                    ),
                    route_id: access.route_id.clone(),
                    api_key_id: access.api_key_id.clone(),
                    model: access.model.clone(),
                    provider_id: access.provider_id.clone(),
                    owner_id: owner_id.clone(),
                };
                let entry = buffer.entry(key).or_default();
                entry.requests += 1;
                if is_error {
                    entry.errors += 1;
                }
                entry.client_errors += client_error;
                entry.server_errors += server_error;
                entry.failovers += failovers;
                entry.bytes_in += access.bytes_in as i64;
                if access.ttfb_ms > 0 {
                    entry.ttft_sum_ms += access.ttfb_ms as i64;
                    entry.ttft_count += 1;
                    entry.latency_sum_ms += access.ttfb_ms as i64;
                    entry.latency_count += 1;
                }
                for (index, value) in hist.iter().enumerate() {
                    entry.hist[index] += value;
                }
            }
        }

        let keep = match settings.detail_mode {
            DetailMode::Off => false,
            // `errors` 同时保留"发生过换号"的请求：最终可能 200，
            // 但 attempts_json 是排查换号的关键数据。
            DetailMode::Errors => is_error || failovers > 0,
            DetailMode::All => rand::random::<f64>() < settings.detail_sample_rate,
        };
        if !keep {
            return;
        }

        let attempts_json = serde_json::to_string(
            &attempts
                .iter()
                .map(|attempt| {
                    serde_json::json!({
                        "provider_id": attempt.provider_id,
                        "status": attempt.status,
                        "error": attempt.error,
                        "latency_ms": attempt.latency_ms,
                    })
                })
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string());

        let row = crate::db::detail::DetailInsert {
            request_id: access.request_id.clone(),
            at: access.at,
            route_id: access.route_id.clone(),
            api_key_id: access.api_key_id.clone(),
            owner_id,
            model: access.model.clone(),
            provider_id: access.provider_id.clone(),
            api_format: access.api_format.clone(),
            is_stream: access.is_stream,
            status: access.status as i64,
            attempts: access.attempts as i64,
            failovers,
            latency_ms: access.ttfb_ms as i64,
            ttft_ms: (access.ttfb_ms > 0).then_some(access.ttfb_ms as i64),
            bytes_in: access.bytes_in as i64,
            attempts_json,
        };
        // 明细进缓冲，由批量任务（每 2.5s）单事务落库。
        let request_id = row.request_id.clone();
        let mut buffer = self.detail_buffer.lock().await;
        buffer.entry(request_id).or_default().insert = Some(row);
    }

    /// 记录一次 provider 尝试（尝试桶）。
    pub async fn record_attempt(&self, attempt: &AttemptEvent) {
        let settings = self.settings.lock().await.clone();
        if !settings.enabled {
            return;
        }
        let success = matches!(attempt.status, Some(status) if (200..300).contains(&status));
        let error = !success;
        let (status_4xx, status_5xx) = attempt.status.map(status_class).unwrap_or((0, 0));
        let mut buffer = self.attempt_buffer.lock().await;
        for granularity in ["hour", "day"] {
            let key = AttemptKey {
                granularity,
                bucket_ts: bucket_start(attempt.at, granularity, settings.timezone_offset_minutes),
                route_id: attempt.route_id.clone(),
                provider_id: attempt.provider_id.clone(),
                model: attempt.model.clone(),
            };
            let entry = buffer.entry(key).or_default();
            entry.attempts += 1;
            if success {
                entry.success += 1;
            }
            if error {
                entry.errors += 1;
            }
            entry.status_4xx += status_4xx;
            entry.status_5xx += status_5xx;
            entry.latency_sum_ms += attempt.latency_ms as i64;
            entry.latency_count += 1;
        }
    }

    /// 记录一次逻辑请求的完成事件（token / bytes_out / outcome）。
    pub async fn record_completion(&self, completion: &CompletionEvent) {
        let settings = self.settings.lock().await.clone();
        if !settings.enabled {
            return;
        }
        let owner_id = self.owner_for(&completion.route_id).await;
        let usage = completion.usage.unwrap_or_default();
        let mut delta = BucketDelta {
            duration_sum_ms: completion.duration_ms as i64,
            duration_count: 1,
            bytes_out: completion.bytes_out as i64,
            input_tokens: usage.input_tokens as i64,
            output_tokens: usage.output_tokens as i64,
            cached_tokens: usage.cached_tokens as i64,
            reasoning_tokens: usage.reasoning_tokens as i64,
            cache_write_tokens: usage.cache_write_tokens as i64,
            ..Default::default()
        };
        match completion.outcome {
            CompletionOutcome::Completed => {
                delta.completed = 1;
                // 非生成端点（count_tokens / countTokens）本就没有 usage，
                // 计入「用量缺失」会污染「上游没给 usage」这个信号。
                if completion.usage.is_none() && completion.generation {
                    delta.usage_missing = 1;
                }
            }
            CompletionOutcome::Aborted => delta.aborted = 1,
            CompletionOutcome::UpstreamError => delta.stream_errors = 1,
        }

        {
            let mut buffer = self.buffer.lock().await;
            for granularity in ["hour", "day"] {
                let key = Key {
                    granularity,
                    bucket_ts: bucket_start(
                        completion.at,
                        granularity,
                        settings.timezone_offset_minutes,
                    ),
                    route_id: completion.route_id.clone(),
                    api_key_id: completion.api_key_id.clone(),
                    model: completion.model.clone(),
                    provider_id: completion.provider_id.clone(),
                    owner_id: owner_id.clone(),
                };
                let entry = buffer.entry(key).or_default();
                entry.completed += delta.completed;
                entry.aborted += delta.aborted;
                entry.stream_errors += delta.stream_errors;
                entry.usage_missing += delta.usage_missing;
                entry.input_tokens += delta.input_tokens;
                entry.output_tokens += delta.output_tokens;
                entry.cached_tokens += delta.cached_tokens;
                entry.reasoning_tokens += delta.reasoning_tokens;
                entry.cache_write_tokens += delta.cache_write_tokens;
                entry.duration_sum_ms += delta.duration_sum_ms;
                entry.duration_count += delta.duration_count;
                entry.bytes_out += delta.bytes_out;
            }
        }

        // 明细关闭或明细库不可用时不必缓冲完成信息。
        if self.detail.is_none() || matches!(settings.detail_mode, DetailMode::Off) {
            return;
        }
        let done = crate::db::detail::DetailCompletion {
            outcome: completion.outcome.as_str().to_string(),
            duration_ms: completion.duration_ms as i64,
            bytes_out: completion.bytes_out as i64,
            input_tokens: usage.input_tokens as i64,
            output_tokens: usage.output_tokens as i64,
            cached_tokens: usage.cached_tokens as i64,
            reasoning_tokens: usage.reasoning_tokens as i64,
            cache_write_tokens: usage.cache_write_tokens as i64,
        };
        let mut buffer = self.detail_buffer.lock().await;
        buffer
            .entry(completion.request_id.clone())
            .or_default()
            .completion = Some(done);
    }

    /// 启动定期落盘与保留清理。
    pub fn spawn_tasks(self: &Arc<Self>) {
        let flush_collector = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                flush_collector.flush().await;
            }
        });

        let detail_collector = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(DETAIL_FLUSH_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                detail_collector.flush_details().await;
            }
        });

        let upkeep_collector = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                upkeep_collector.refresh_settings().await;
                if let Err(error) = upkeep_collector.cleanup().await {
                    tracing::error!(%error, "usage retention cleanup failed");
                }
            }
        });
    }

    /// 落盘一次：同一批聚合桶放在一个事务里；失败整批回灌，下个周期重试。
    pub(crate) async fn flush(&self) {
        let drained: Vec<(Key, BucketDelta)> = {
            let mut buffer = self.buffer.lock().await;
            buffer.drain().collect()
        };
        if !drained.is_empty() && !self.flush_bucket_batch(&drained).await {
            let mut buffer = self.buffer.lock().await;
            for (key, delta) in drained {
                buffer.entry(key).or_default().merge(delta);
            }
        }

        let drained_attempts: Vec<(AttemptKey, AttemptDelta)> = {
            let mut buffer = self.attempt_buffer.lock().await;
            buffer.drain().collect()
        };
        if !drained_attempts.is_empty() && !self.flush_attempt_batch(&drained_attempts).await {
            let mut buffer = self.attempt_buffer.lock().await;
            for (key, delta) in drained_attempts {
                buffer.entry(key).or_default().merge(delta);
            }
        }
    }

    async fn flush_bucket_batch(&self, rows: &[(Key, BucketDelta)]) -> bool {
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::error!(%error, "failed to begin usage aggregate transaction; retrying later");
                return false;
            }
        };
        for (key, delta) in rows {
            if let Err(error) = usage::add_bucket_on(
                &mut *tx,
                key.granularity,
                key.bucket_ts,
                &key.route_id,
                &key.api_key_id,
                &key.model,
                &key.provider_id,
                &key.owner_id,
                delta,
            )
            .await
            {
                tracing::error!(%error, "failed to persist usage aggregate; retrying later");
                let _ = tx.rollback().await;
                return false;
            }
        }
        match tx.commit().await {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, "failed to commit usage aggregate transaction; retrying later");
                false
            }
        }
    }

    async fn flush_attempt_batch(&self, rows: &[(AttemptKey, AttemptDelta)]) -> bool {
        let mut tx = match self.pool.begin().await {
            Ok(tx) => tx,
            Err(error) => {
                tracing::error!(%error, "failed to begin attempt aggregate transaction; retrying later");
                return false;
            }
        };
        for (key, delta) in rows {
            if let Err(error) = usage::add_attempt_bucket_on(
                &mut *tx,
                key.granularity,
                key.bucket_ts,
                &key.route_id,
                &key.provider_id,
                &key.model,
                delta,
            )
            .await
            {
                tracing::error!(%error, "failed to persist attempt aggregate; retrying later");
                let _ = tx.rollback().await;
                return false;
            }
        }
        match tx.commit().await {
            Ok(()) => true,
            Err(error) => {
                tracing::error!(%error, "failed to commit attempt aggregate transaction; retrying later");
                false
            }
        }
    }

    /// 明细批量落库：一个事务写完整批；失败整批回灌缓冲。
    async fn flush_details(&self) {
        let Some(store) = self.detail.as_ref() else {
            // 明细库不可用：丢弃缓冲，避免无界增长。
            self.detail_buffer.lock().await.clear();
            return;
        };
        let drained: Vec<(String, PendingDetail)> = {
            let mut buffer = self.detail_buffer.lock().await;
            buffer.drain().collect()
        };
        if drained.is_empty() {
            return;
        }

        let mut upserts: Vec<(
            crate::db::detail::DetailInsert,
            Option<crate::db::detail::DetailCompletion>,
        )> = Vec::new();
        let mut updates: Vec<(String, crate::db::detail::DetailCompletion)> = Vec::new();
        let mut leftovers: Vec<(String, PendingDetail)> = Vec::new();
        for (request_id, pending) in drained {
            match (pending.insert, pending.completion.clone()) {
                (Some(insert), completion) => upserts.push((insert, completion)),
                // 该行已在之前批次落库：只补完成信息。
                (None, Some(completion)) => updates.push((request_id, completion)),
                (None, None) => leftovers.push((
                    request_id,
                    PendingDetail {
                        insert: None,
                        completion: None,
                    },
                )),
            }
        }

        let mut failed: Vec<(String, PendingDetail)> = Vec::new();
        if !upserts.is_empty()
            && let Err(error) = crate::db::detail::upsert_batch(store.pool(), &upserts).await
        {
            tracing::error!(%error, "failed to persist request details; retrying later");
            for (insert, completion) in upserts {
                failed.push((
                    insert.request_id.clone(),
                    PendingDetail {
                        insert: Some(insert),
                        completion,
                    },
                ));
            }
        }
        if !updates.is_empty()
            && let Err(error) =
                crate::db::detail::update_completion_batch(store.pool(), &updates).await
        {
            tracing::error!(%error, "failed to update detail completions; retrying later");
            for (request_id, completion) in updates {
                failed.push((
                    request_id,
                    PendingDetail {
                        insert: None,
                        completion: Some(completion),
                    },
                ));
            }
        }

        let requeue = failed.len() + leftovers.len();
        if requeue > 0 {
            let mut buffer = self.detail_buffer.lock().await;
            for (request_id, pending) in failed.into_iter().chain(leftovers) {
                let entry = buffer.entry(request_id).or_default();
                if let Some(insert) = pending.insert {
                    entry.insert = Some(insert);
                }
                if let Some(completion) = pending.completion {
                    entry.completion = Some(completion);
                }
            }
        }
    }

    /// 立即落盘一次（进程收尾用）：聚合桶 + 明细。
    pub async fn flush_now(&self) {
        self.flush().await;
        self.flush_details().await;
    }

    /// 从主库刷新路由归属缓存（路由创建 / 转移 / offboard 后调用）。
    pub async fn refresh_owners(&self) {
        match routes::owner_map(&self.pool).await {
            Ok(map) => *self.owners.lock().await = map,
            Err(error) => tracing::error!(%error, "failed to refresh route owners"),
        }
    }

    async fn owner_for(&self, route_id: &str) -> String {
        self.owners
            .lock()
            .await
            .get(route_id)
            .cloned()
            .unwrap_or_default()
    }

    /// 刷新缓存的用量设置（设置变更时由控制面调用）。
    pub async fn refresh_settings(&self) {
        match settings::load_usage(&self.pool).await {
            Ok(loaded) => *self.settings.lock().await = loaded,
            Err(error) => tracing::error!(%error, "failed to refresh usage settings"),
        }
    }

    async fn cleanup(&self) -> Result<()> {
        let now = now_unix();
        let detail_before = now - (self.settings.lock().await.detail_retention_hours as i64) * 3600;
        usage::delete_old(
            &self.pool,
            now - HOUR_RETENTION_DAYS * 24 * 3600,
            now - DAY_RETENTION_DAYS * 24 * 3600,
        )
        .await?;
        if let Some(store) = self.detail.clone() {
            crate::db::detail::delete_before(store.pool(), detail_before).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteConnectOptions;

    async fn pool_without_usage_tables() -> sqlx::SqlitePool {
        // 不跑迁移：`usage_bucket` 不存在 → 落盘必然失败，用于验证回灌。
        sqlx::SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(":memory:")
                .create_if_missing(true),
        )
        .await
        .expect("pool")
    }

    async fn collector(pool: sqlx::SqlitePool) -> Arc<UsageCollector> {
        // settings 保持默认（enabled = true）。
        Arc::new(UsageCollector::new(pool, None).await)
    }

    fn delta(requests: i64) -> BucketDelta {
        BucketDelta {
            requests,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn failed_flush_requeues_deltas() {
        let collector = collector(pool_without_usage_tables().await).await;
        {
            let mut buffer = collector.buffer.lock().await;
            let key = Key {
                granularity: "hour",
                bucket_ts: 1000,
                route_id: "r1".into(),
                api_key_id: "k1".into(),
                model: "m".into(),
                provider_id: "p1".into(),
                owner_id: "u1".into(),
            };
            *buffer.entry(key).or_default() = delta(3);
        }

        collector.flush().await;

        // 落库失败 → 增量回到缓冲，未丢失。
        let buffer = collector.buffer.lock().await;
        let total: i64 = buffer.values().map(|value| value.requests).sum();
        assert_eq!(total, 3);
    }

    #[tokio::test]
    async fn failed_attempt_flush_requeues_deltas() {
        let collector = collector(pool_without_usage_tables().await).await;
        {
            let mut buffer = collector.attempt_buffer.lock().await;
            let key = AttemptKey {
                granularity: "hour",
                bucket_ts: 1000,
                route_id: "r1".into(),
                provider_id: "p1".into(),
                model: "m".into(),
            };
            *buffer.entry(key).or_default() = AttemptDelta {
                attempts: 2,
                success: 1,
                errors: 1,
                ..Default::default()
            };
        }

        collector.flush().await;

        let buffer = collector.attempt_buffer.lock().await;
        let attempts: i64 = buffer.values().map(|value| value.attempts).sum();
        assert_eq!(attempts, 2);
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;
    use crate::db::detail::DetailStore;
    use solidway_core::{AccessEvent, CompletionEvent, CompletionOutcome};

    fn access(request_id: &str) -> AccessEvent {
        AccessEvent {
            request_id: request_id.to_string(),
            at: 1000,
            route_id: "r1".into(),
            api_key_id: "k1".into(),
            model: "m".into(),
            provider_id: "p1".into(),
            api_format: "chat_completions".into(),
            is_stream: false,
            status: 200,
            attempts: 2,
            ttfb_ms: 120,
            bytes_in: 10,
        }
    }

    fn completion(request_id: &str) -> CompletionEvent {
        CompletionEvent {
            request_id: request_id.to_string(),
            at: 1000,
            route_id: "r1".into(),
            api_key_id: "k1".into(),
            model: "m".into(),
            provider_id: "p1".into(),
            outcome: CompletionOutcome::Completed,
            duration_ms: 800,
            bytes_out: 20,
            usage: Some(solidway_core::TokenUsage {
                input_tokens: 100,
                output_tokens: 20,
                ..Default::default()
            }),
            generation: true,
        }
    }

    async fn collector_with_detail() -> (Arc<UsageCollector>, DetailStore, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("solidway-detail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let detail = DetailStore::open(&dir).await.expect("detail store");
        let main = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(":memory:")
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(main).await.unwrap();
        let collector = Arc::new(UsageCollector::new(pool, Some(detail.clone())).await);
        (collector, detail, dir)
    }

    async fn rows(store: &DetailStore) -> Vec<(String, i64, String, i64)> {
        sqlx::query_as(
            "SELECT request_id, input_tokens, outcome, bytes_out FROM usage_request_detail",
        )
        .fetch_all(store.pool())
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn access_and_completion_merge_into_one_row() {
        let (collector, store, dir) = collector_with_detail().await;
        collector.record(&access("req-1"), &[]).await;
        collector.record_completion(&completion("req-1")).await;
        collector.flush_details().await;

        let rows = rows(&store).await;
        assert_eq!(rows.len(), 1, "同一请求应只落一行：{rows:?}");
        assert_eq!(rows[0].1, 100, "token 应回填");
        assert_eq!(rows[0].2, "completed");
        assert_eq!(rows[0].3, 20);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn completion_after_flush_updates_existing_row() {
        let (collector, store, dir) = collector_with_detail().await;
        collector.record(&access("req-2"), &[]).await;
        collector.flush_details().await;
        // 该行已落库，随后完成事件才到 → 走 UPDATE 分支。
        collector.record_completion(&completion("req-2")).await;
        collector.flush_details().await;

        let rows = rows(&store).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, 100);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn detail_flush_failure_requeues() {
        let (collector, store, dir) = collector_with_detail().await;
        // 让落库必然失败：删掉明细表。
        sqlx::query("DROP TABLE usage_request_detail")
            .execute(store.pool())
            .await
            .unwrap();
        collector.record(&access("req-3"), &[]).await;
        collector.flush_details().await;

        assert_eq!(
            collector.detail_buffer.lock().await.len(),
            1,
            "失败后应回灌缓冲"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn detail_off_buffers_nothing() {
        let (collector, store, dir) = collector_with_detail().await;
        collector.settings.lock().await.detail_mode = DetailMode::Off;
        collector.record(&access("req-4"), &[]).await;
        collector.record_completion(&completion("req-4")).await;
        assert!(collector.detail_buffer.lock().await.is_empty());
        collector.flush_details().await;
        assert!(rows(&store).await.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
