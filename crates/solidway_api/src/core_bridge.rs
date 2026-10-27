//! 控制面 ↔ 数据面（`solidway_core`）的桥接。
//!
//! - 下发：把 DB 中的路由 / provider 凭据 / API Key / 健康种子构造为 [`CoreConfig`]，
//!   provider 的 api_key 在此**解密**后交给 core（core 不落盘、不记日志）。
//! - 上收：订阅 [`CoreEvent`]，持久化 provider 健康状态并采集用量。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use solidway_core::config::DEFAULT_REQUEST_BODY_LIMIT;
use solidway_core::{
    ApiFormat, ApiKeyEntry, Cooldowns, Core, CoreConfig, CoreEvent, CoreOptions, HealthState,
    ModelMapping, ProviderEntry, ProviderHealthSeed, RouteEntry,
};

use crate::db::{api_keys, provider_health, provider_keys, routes};
use crate::error::{ApiError, Result};
use crate::settings;
use crate::state::AppState;

fn parse_models(raw: &str) -> Vec<ModelMapping> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn parse_api_formats(raw: &str) -> Vec<ApiFormat> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| ApiFormat::parse(&value))
        .collect()
}

/// 从 DB 构造 core 的完整配置（**运行期下发**：不带任何种子）。
pub async fn build_core_config(state: &AppState) -> Result<CoreConfig> {
    build_config(state, false).await
}

/// 启动装配用的配置：在运行期配置之上附带**一次性种子**。
///
/// - 选号指标：从最近 30 分钟的 `usage_attempt_bucket` 聚合；
/// - 会话粘性：取自 `runtime_snapshot`（过滤已过期条目）。
pub async fn build_startup_config(state: &AppState) -> Result<CoreConfig> {
    build_config(state, true).await
}

async fn build_config(state: &AppState, with_seeds: bool) -> Result<CoreConfig> {
    let route_rows = routes::list(&state.pool).await?;
    let provider_rows = provider_keys::list_all(&state.pool).await?;
    let key_rows = api_keys::list_all_rows(&state.pool).await?;
    let health_rows = provider_health::load_all(&state.pool).await?;
    // 直接读库：`build_core_config` 本就是 async 下发路径，保证与库一致。
    let gateway = settings::load_gateway(&state.pool).await?;
    let network = settings::load_network(&state.pool).await?;

    let routes_cfg: Vec<RouteEntry> = route_rows
        .into_iter()
        .map(|route| {
            let (strategy, overrides) =
                crate::selection::from_row(&route.selection_strategy, &route.selection_config);
            let resolved = crate::selection::resolve(strategy, &overrides);
            RouteEntry {
                id: route.id,
                name: route.name,
                enabled: route.enabled != 0,
                selection: solidway_core::RouteSelection {
                    strategy: resolved.strategy,
                    weights: resolved.weights,
                    thresholds: resolved.thresholds,
                },
            }
        })
        .collect();

    let mut providers_cfg = Vec::with_capacity(provider_rows.len());
    for row in provider_rows {
        let aad = format!("provider_key|{}", row.id);
        let api_key = state.cipher.decrypt(&row.credential_ciphertext, &aad)?;
        // 以记录上的显式鉴权方式为准；空值回退到所属预设。
        let auth_scheme = if row.auth_scheme.trim().is_empty() {
            crate::provider::presets::preset_of(&row.provider)
                .map(|preset| preset.auth_scheme.clone())
                .unwrap_or_else(|| "bearer".to_string())
        } else {
            row.auth_scheme.clone()
        };
        providers_cfg.push(ProviderEntry {
            id: row.id,
            route_id: row.route_id,
            name: row.name,
            provider: row.provider,
            base_url: row.base_url.clone().unwrap_or_default(),
            api_key,
            credential_kind: row.credential_kind,
            auth_scheme,
            options: row
                .options
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok()),
            api_formats: parse_api_formats(&row.api_formats),
            models: parse_models(&row.models),
            enabled: row.status == "active",
            // 防御：schema 已限制 1..=1000，出现异常值时按最低档处理。
            priority: row.priority.max(1),
        });
    }

    let api_keys_cfg: Vec<ApiKeyEntry> = key_rows
        .into_iter()
        .map(|key| ApiKeyEntry {
            id: key.id,
            route_id: key.route_id,
            key_hash: key.key_hash,
            enabled: key.status == "active",
            expires_at: key.expires_at,
        })
        .collect();

    let health_cfg: Vec<ProviderHealthSeed> = health_rows
        .into_iter()
        .filter(|row| {
            providers_cfg
                .iter()
                .any(|provider| provider.id == row.provider_id)
        })
        .map(|row| ProviderHealthSeed {
            provider_id: row.provider_id,
            state: if row.state == "retired" {
                HealthState::Retired { reason: row.reason }
            } else {
                HealthState::Quarantined {
                    until: row.cooldown_until.unwrap_or(0),
                    reason: row.reason,
                }
            },
        })
        .collect();

    let options = CoreOptions {
        connect_timeout: Duration::from_secs(network.connect_timeout_secs),
        first_byte_timeout: Duration::from_secs(network.first_byte_timeout_secs),
        // 响应体读取单独限时（不跟随首字节的超长配置），避免故障上游挂住换号或探针。
        response_body_timeout: Duration::from_secs(10),
        cooldowns: Cooldowns {
            auth: Duration::from_secs(gateway.cooldown_auth_secs),
            rate_limit: Duration::from_secs(gateway.cooldown_rate_limit_secs),
            unavailable: Duration::from_secs(gateway.cooldown_unavailable_secs),
            quota: Duration::from_secs(gateway.quota_cooldown_secs),
        },
        affinity_ttl: Duration::from_secs(gateway.affinity_ttl_secs),
        request_body_limit: DEFAULT_REQUEST_BODY_LIMIT,
        response_body_limit: DEFAULT_REQUEST_BODY_LIMIT,
    };

    let (metrics, affinity) = if with_seeds {
        (
            metric_seeds(state).await?,
            crate::db::runtime_snapshot::load(&state.pool)
                .await
                .affinity,
        )
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(CoreConfig {
        api_keys: api_keys_cfg,
        routes: routes_cfg,
        providers: providers_cfg,
        health: health_cfg,
        metrics,
        affinity,
        options,
    })
}

/// 与用量桶一致的小时对齐（本地时区偏移）。
fn hour_start(ts: i64, offset_minutes: i64) -> i64 {
    let offset = offset_minutes * 60;
    ts - (ts + offset).rem_euclid(3600)
}

/// 选号指标种子：最近 30 分钟的尝试聚合（每个 provider 一条，无样本的不发）。
///
/// 口径：`failure_rate = 1 − success / attempts`（与 llmgateway 的 uptime 接近；
/// 请求级 4xx 也会拉低该值，属已知偏差，见 change 的 design）。
async fn metric_seeds(state: &AppState) -> Result<Vec<solidway_core::ProviderMetricSeed>> {
    let now = crate::state::now_unix();
    // 与数据面的 30 分钟陈旧判定对齐；桶按本地时区对齐，所以窗口也要先落到小时边界，
    // 否则"当前小时桶的起点"可能早于 now-30min 而被过滤掉。
    let usage = crate::settings::load_usage(&state.pool).await?;
    let since = hour_start(now - 30 * 60, usage.timezone_offset_minutes);
    let rows = sqlx::query_as::<_, (String, i64, i64, i64, i64)>(
        "SELECT provider_id,
                SUM(attempts)       AS attempts,
                SUM(success)        AS success,
                SUM(latency_sum_ms) AS latency_sum,
                SUM(latency_count)  AS latency_count
         FROM usage_attempt_bucket
         WHERE granularity = 'hour' AND bucket_ts >= ?1
         GROUP BY provider_id",
    )
    .bind(since)
    .fetch_all(&state.pool)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(
            |(provider_id, attempts, success, latency_sum, latency_count)| {
                if attempts <= 0 || latency_count <= 0 {
                    return None;
                }
                let ttfb_ms = latency_sum as f64 / latency_count as f64;
                let failure_rate = (1.0 - success as f64 / attempts as f64).clamp(0.0, 1.0);
                Some(solidway_core::ProviderMetricSeed {
                    provider_id,
                    ttfb_ms,
                    failure_rate,
                    samples: attempts as u64,
                })
            },
        )
        .collect())
}

/// 常驻消费 core 事件：持久化健康；采集用量。
///
/// `Attempt` 与 `Access` 属同一 `request_id`，先在内存按请求聚合尝试列表，
/// 收到 `Access` 时一并交给采集器（尝试明细随请求落库）。
pub fn spawn_event_pump(
    state: AppState,
    core: Arc<Core>,
    collector: Arc<crate::usage::UsageCollector>,
) {
    let mut rx = core.subscribe();
    tokio::spawn(async move {
        let mut pending: HashMap<String, Vec<solidway_core::AttemptEvent>> = HashMap::new();
        loop {
            match rx.recv().await {
                Ok(CoreEvent::HealthChanged(change)) => persist_health(&state, change).await,
                Ok(CoreEvent::Attempt(attempt)) => {
                    // 无 `Access` 时也不会无限增长：超过阈值直接清空（异常保护）。
                    if pending.len() > 10_000 {
                        pending.clear();
                    }
                    collector.record_attempt(&attempt).await;
                    pending
                        .entry(attempt.request_id.clone())
                        .or_default()
                        .push(attempt);
                }
                Ok(CoreEvent::Access(access)) => {
                    let attempts = pending.remove(&access.request_id).unwrap_or_default();
                    collector.record(&access, &attempts).await;
                }
                Ok(CoreEvent::Completion(completion)) => {
                    collector.record_completion(&completion).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "event consumer lagging; events dropped");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

async fn persist_health(state: &AppState, change: solidway_core::HealthChangedEvent) {
    persist_incident(state, &change).await;
    let result = match &change.state {
        HealthState::Healthy => provider_health::delete(&state.pool, &change.provider_id).await,
        HealthState::Quarantined { until, reason } => {
            provider_health::upsert(
                &state.pool,
                &change.provider_id,
                "quarantined",
                reason,
                Some(*until),
                change.at,
            )
            .await
        }
        HealthState::Retired { reason } => {
            provider_health::upsert(
                &state.pool,
                &change.provider_id,
                "retired",
                reason,
                None,
                change.at,
            )
            .await
        }
    };
    if let Err(error) = result {
        tracing::error!(%error, provider = %change.provider_id, "failed to persist provider health");
    }
}

/// 把健康迁移落成故障事件（incident）：标记 → 开/合并；回池 → 关闭。
async fn persist_incident(state: &AppState, change: &solidway_core::HealthChangedEvent) {
    let route_id: Option<(String,)> =
        sqlx::query_as("SELECT route_id FROM provider_keys WHERE id = ?1")
            .bind(&change.provider_id)
            .fetch_optional(&state.pool)
            .await
            .ok()
            .flatten();

    let Some((route_id,)) = route_id else {
        // 凭据已删除：不再产生新事件。
        return;
    };

    match &change.state {
        HealthState::Healthy => {
            match crate::db::incidents::close_open(
                &state.pool,
                &change.provider_id,
                change.at,
                "auto_probe",
            )
            .await
            {
                Ok(closed) if !closed.is_empty() => {
                    crate::alerts::notify_closed(state, &closed).await;
                }
                Ok(_) => {}
                Err(error) => tracing::error!(%error, "failed to close incident"),
            }
        }
        HealthState::Quarantined { reason, .. } | HealthState::Retired { reason } => {
            let last_status = match reason.as_str() {
                "http_401" => Some(401),
                "http_403" => Some(403),
                "http_429" | "insufficient_quota" => Some(429),
                "http_5xx" => Some(500),
                _ => None,
            };
            match crate::db::incidents::record(
                &state.pool,
                &change.provider_id,
                &route_id,
                reason,
                change.at,
                last_status,
                Some(reason),
            )
            .await
            {
                Ok((id, created)) => {
                    if created {
                        crate::alerts::notify_opened(state, &id).await;
                    }
                }
                Err(error) => tracing::error!(%error, "failed to record incident"),
            }
        }
    }
}

/// 下发串行化（R2-A-2）：build + apply 必须原子完成，避免并发写路径把旧快照
/// 覆盖到新快照上。`tokio::Mutex` 不可重入——**只在 `sync_core` 入口获取**；
/// 持锁路径（`build_core_config` / `apply_config` / 各写 handler）MUST NOT 再调用
/// `sync_core`，否则死锁。管理面低并发，锁竞争可忽略（KISS，不上版本号）。
static SYNC_CORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl AppState {
    /// 把当前 DB 配置重新下发给 core（若已装配）。
    pub async fn sync_core(&self) -> Result<()> {
        let _guard = SYNC_CORE_LOCK.lock().await;
        let core = self.core.read().expect("core lock").clone();
        let Some(core) = core else {
            return Ok(());
        };
        let config = build_core_config(self).await?;
        core.apply_config(config)
            .await
            .map_err(|error| ApiError::internal(format!("core 配置下发失败: {error}")))?;
        Ok(())
    }

    /// 写操作后的尽力下发：失败只记日志，不阻断管理面写操作。
    pub async fn sync_core_best_effort(&self) {
        if let Err(error) = self.sync_core().await {
            tracing::error!(%error, "failed to apply core config");
        }
    }

    /// 设置变更后刷新用量采集器的缓存。
    pub async fn refresh_usage_settings(&self) {
        let collector = self.usage.read().expect("usage lock").clone();
        if let Some(collector) = collector {
            collector.refresh_settings().await;
        }
    }
}
