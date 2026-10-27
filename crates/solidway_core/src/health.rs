//! provider 健康：失败分类与状态类型（纯逻辑，无 DB）。
//!
//! 状态机与持久化分离：本模块只做**分类**与**状态类型**；运行期权威在内存，
//! 变更由控制面订阅 [`crate::CoreEvent::HealthChanged`] 后落库。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::config::ProviderHealthSeed;
use crate::event::{CoreEvent, EventBus, HealthChangedEvent};

/// 当前时刻的 epoch 秒（早于 1970 时回退 0）。
pub fn epoch_secs(now: SystemTime) -> i64 {
    match now.duration_since(UNIX_EPOCH) {
        Ok(delta) => delta.as_secs() as i64,
        Err(_) => 0,
    }
}

/// 健康标记：把失败映射为 provider 级处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// 临时移出（冷却时长由分类决定）。
    Quarantine(Duration),
    /// 永久退役（不再自动回池）。
    Retire,
    /// 请求级错误：不标记 provider。
    None,
}

/// 被标记时记录的机器可读原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkReason {
    /// 401：凭据失效，退役待人工处理。
    Http401,
    /// 403：账号异常，先按冷却处理（未确定封禁）。
    Http403,
    /// 429 且 `error.code` 为 `insufficient_quota`。
    InsufficientQuota,
    /// 429 其他（按 `Retry-After` 或默认限流冷却）。
    Http429,
    /// 5xx。
    Http5xx,
    /// 网络错误 / 连接超时 / 首字节超时。
    Transport,
}

impl MarkReason {
    /// 落库与日志使用的稳定标识。
    pub fn as_str(self) -> &'static str {
        match self {
            MarkReason::Http401 => "http_401",
            MarkReason::Http403 => "http_403",
            MarkReason::InsufficientQuota => "insufficient_quota",
            MarkReason::Http429 => "http_429",
            MarkReason::Http5xx => "http_5xx",
            MarkReason::Transport => "transport_error",
        }
    }
}

/// 一次响应的分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// 上游 2xx：成功（探针据此判定「通过」）。
    Success,
    /// 给 provider 打标记。
    Marked { mark: Mark, reason: MarkReason },
    /// 请求级错误：不标记 provider。
    Unmarked,
}

impl Classification {
    /// 是否需要标记 provider。
    pub fn is_marked(&self) -> bool {
        matches!(self, Classification::Marked { .. })
    }

    /// 标记处置（未标记时为 [`Mark::None`]）。
    pub fn mark(&self) -> Mark {
        match self {
            Classification::Marked { mark, .. } => *mark,
            _ => Mark::None,
        }
    }
}

/// 冷却与限流相关的时长参数。
#[derive(Debug, Clone, Copy)]
pub struct Cooldowns {
    /// 认证类失败冷却（401/403）。
    pub auth: Duration,
    /// 限流类失败缺省冷却（429 无 `Retry-After`）。
    pub rate_limit: Duration,
    /// 上游不可用缺省冷却（5xx / 网络 / 超时）。
    pub unavailable: Duration,
    /// 配额耗尽类失败的长冷却（`insufficient_quota` 且无 `Retry-After`）。
    ///
    /// 很多厂商按日 / 按周清空配额，因此不永久退役，冷却到期后由探针尝试回池。
    pub quota: Duration,
}

/// 上游传输层失败（无 HTTP 响应）。
#[derive(Debug, Clone)]
pub enum TransportError {
    /// 连接超时 / 请求超时 / 首字节超时。
    Timeout(String),
    /// 其他网络错误或连接中断。
    Other(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::Timeout(detail) => write!(f, "上游超时: {detail}"),
            TransportError::Other(detail) => write!(f, "上游传输失败: {detail}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// 分类输入：要么是上游响应，要么是传输失败。
#[derive(Debug)]
pub enum Failure<'a> {
    /// 上游返回了响应。
    Response {
        /// 上游状态码。
        status: u16,
        /// 上游 `Retry-After` 头（原文）。
        retry_after: Option<&'a str>,
        /// 错误体前若干字节（最多 64KB，仅用于分类解析）。
        body_prefix: &'a [u8],
    },
    /// 未能拿到响应。
    Transport(TransportError),
}

/// 失败分类器：状态码 + `error.code` + `Retry-After`（秒数与 HTTP 日期两种格式）。
/// 配额长冷却的上限（7 天）：`Retry-After` 可能给出离谱的大数。
const MAX_QUOTA_COOLDOWN: Duration = Duration::from_secs(7 * 24 * 3600);

pub fn classify(failure: Failure<'_>, cooldowns: Cooldowns, now: SystemTime) -> Classification {
    match failure {
        Failure::Response {
            status,
            retry_after,
            body_prefix,
        } => match status {
            200..300 => Classification::Success,
            // 401：key 失效不可能自愈 → 退役待人工。
            401 => Classification::Marked {
                mark: Mark::Retire,
                reason: MarkReason::Http401,
            },
            // 403：账号异常、封禁未确定 → 先冷却换号。
            403 => Classification::Marked {
                mark: Mark::Quarantine(cooldowns.auth),
                reason: MarkReason::Http403,
            },
            429 => {
                if error_code(body_prefix).as_deref() == Some("insufficient_quota") {
                    // 配额可能按周期恢复：Retry-After 优先，否则长冷却。
                    let cooldown = retry_after
                        .and_then(|raw| parse_retry_after(raw, now))
                        .unwrap_or(cooldowns.quota)
                        .min(MAX_QUOTA_COOLDOWN);
                    Classification::Marked {
                        mark: Mark::Quarantine(cooldown),
                        reason: MarkReason::InsufficientQuota,
                    }
                } else {
                    let cooldown = retry_after
                        .and_then(|raw| parse_retry_after(raw, now))
                        .unwrap_or(cooldowns.rate_limit);
                    Classification::Marked {
                        mark: Mark::Quarantine(cooldown),
                        reason: MarkReason::Http429,
                    }
                }
            }
            status if (500..600).contains(&status) => Classification::Marked {
                mark: Mark::Quarantine(cooldowns.unavailable),
                reason: MarkReason::Http5xx,
            },
            _ => Classification::Unmarked,
        },
        Failure::Transport(TransportError::Timeout(_))
        | Failure::Transport(TransportError::Other(_)) => Classification::Marked {
            mark: Mark::Quarantine(cooldowns.unavailable),
            reason: MarkReason::Transport,
        },
    }
}

/// 从错误体取 `error.code`（OpenAI 风格错误体）。
fn error_code(body_prefix: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body_prefix).ok()?;
    let code = value.get("error")?.get("code")?;
    match code {
        serde_json::Value::String(text) => Some(text.clone()),
        other => other.as_str().map(str::to_string),
    }
}

/// 解析 `Retry-After`：纯秒数，或 HTTP 日期（RFC 7231 IMF-fixdate）。
/// 解析失败、或日期已过去时返回 `None`。
pub fn parse_retry_after(raw: &str, now: SystemTime) -> Option<Duration> {
    let raw = raw.trim();
    if let Ok(secs) = raw.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let target = parse_http_date(raw)?;
    let now = epoch_secs(now);
    if target > now {
        Some(Duration::from_secs((target - now) as u64))
    } else {
        None
    }
}

/// 解析 IMF-fixdate（`Wed, 21 Oct 2015 07:28:00 GMT`）为 epoch 秒。
fn parse_http_date(raw: &str) -> Option<i64> {
    let rest = raw.split_once(", ").map(|(_, rest)| rest).unwrap_or(raw);
    let mut parts = rest.split_whitespace();
    let day: i64 = parts.next()?.parse().ok()?;
    let month = month_from_name(parts.next()?)?;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next()?.parse().ok()?;
    let zone = parts.next()?;
    if !zone.eq_ignore_ascii_case("GMT") && !zone.eq_ignore_ascii_case("UTC") {
        return None;
    }
    if parts.next().is_some() || clock.next().is_some() {
        return None;
    }
    if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second)
}

fn month_from_name(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS
        .iter()
        .position(|month| month.eq_ignore_ascii_case(name))
        .map(|idx| idx as i64 + 1)
}

/// 公历日期 → 1970-01-01 起的天数（Howard Hinnant 的 days_from_civil）。
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// provider 健康状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum HealthState {
    /// 健康，可参与选号。
    Healthy,
    /// 临时移出：`until` 为冷却截止时刻；冷却到期的 provider 仍不可选，须先通过探针。
    Quarantined { until: i64, reason: String },
    /// 永久退役，不自动回池（人工清库 + 重启恢复）。
    Retired { reason: String },
}

impl HealthState {
    /// 落库 / 接口使用的稳定标识（与 `provider_health.state` 一致）。
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthState::Healthy => "healthy",
            HealthState::Quarantined { .. } => "quarantined",
            HealthState::Retired { .. } => "retired",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cooldowns() -> Cooldowns {
        Cooldowns {
            auth: Duration::from_secs(1800),
            rate_limit: Duration::from_secs(300),
            unavailable: Duration::from_secs(60),
            quota: Duration::from_secs(6 * 3600),
        }
    }

    fn resp<'a>(status: u16, retry_after: Option<&'a str>, body: &'a [u8]) -> Failure<'a> {
        Failure::Response {
            status,
            retry_after,
            body_prefix: body,
        }
    }

    #[test]
    fn auth_failure_retires() {
        let c = classify(resp(401, None, b""), cooldowns(), SystemTime::now());
        assert_eq!(
            c,
            Classification::Marked {
                mark: Mark::Retire,
                reason: MarkReason::Http401
            }
        );
    }

    #[test]
    fn insufficient_quota_takes_long_cooldown() {
        let body = br#"{"error":{"code":"insufficient_quota"}}"#;
        let c = classify(resp(429, None, body), cooldowns(), SystemTime::now());
        assert_eq!(c.mark(), Mark::Quarantine(Duration::from_secs(6 * 3600)));
        assert_eq!(
            c,
            Classification::Marked {
                mark: Mark::Quarantine(Duration::from_secs(6 * 3600)),
                reason: MarkReason::InsufficientQuota
            }
        );

        // Retry-After 优先。
        let c = classify(
            resp(429, Some("7200"), body),
            cooldowns(),
            SystemTime::now(),
        );
        assert_eq!(c.mark(), Mark::Quarantine(Duration::from_secs(7200)));

        // 超出上限时按 7 天截断。
        let c = classify(
            resp(429, Some("99999999"), body),
            cooldowns(),
            SystemTime::now(),
        );
        assert_eq!(
            c.mark(),
            Mark::Quarantine(Duration::from_secs(7 * 24 * 3600))
        );
    }

    #[test]
    fn rate_limit_uses_retry_after() {
        let c = classify(resp(429, Some("120"), b""), cooldowns(), SystemTime::now());
        assert_eq!(c.mark(), Mark::Quarantine(Duration::from_secs(120)));
    }

    #[test]
    fn rate_limit_defaults_without_header() {
        let c = classify(resp(429, None, b""), cooldowns(), SystemTime::now());
        assert_eq!(c.mark(), Mark::Quarantine(Duration::from_secs(300)));
    }

    #[test]
    fn server_error_quarantines_unavailable() {
        let c = classify(resp(503, None, b""), cooldowns(), SystemTime::now());
        assert_eq!(
            c,
            Classification::Marked {
                mark: Mark::Quarantine(Duration::from_secs(60)),
                reason: MarkReason::Http5xx
            }
        );
    }

    #[test]
    fn request_level_error_is_unmarked() {
        let c = classify(resp(400, None, b""), cooldowns(), SystemTime::now());
        assert_eq!(c, Classification::Unmarked);
        assert!(!c.is_marked());
    }

    #[test]
    fn transport_error_quarantines() {
        let c = classify(
            Failure::Transport(TransportError::Timeout("connect".into())),
            cooldowns(),
            SystemTime::now(),
        );
        assert_eq!(c.mark(), Mark::Quarantine(Duration::from_secs(60)));
    }

    #[test]
    fn parse_retry_after_seconds_and_http_date() {
        assert_eq!(
            parse_retry_after(" 30 ", SystemTime::now()),
            Some(Duration::from_secs(30))
        );
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        // 1 小时后（HTTP 日期）
        let later = 1_700_003_600;
        let date = http_date(later);
        assert_eq!(
            parse_retry_after(&date, now),
            Some(Duration::from_secs(3600))
        );
        // 已过去的日期 → None
        let past = http_date(1_699_999_000);
        assert_eq!(parse_retry_after(&past, now), None);
    }

    /// 仅测试用：epoch 秒 → IMF-fixdate。
    fn http_date(secs: i64) -> String {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);
        // days_from_civil 的逆（civil_from_days）
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "Wed, {:02} {} {} {:02}:{:02}:{:02} GMT",
            d,
            MONTHS[(m - 1) as usize],
            y,
            hour,
            minute,
            second
        )
    }
}

// ---------------------------------------------------------------------------
// 健康状态机（纯内存）：运行期权威；状态变更经事件流交给控制面持久化。
// ---------------------------------------------------------------------------

/// provider 健康状态机。
///
/// 与 develop 的差异：**不写库**。每次状态迁移发出 [`CoreEvent::HealthChanged`]，
/// 由控制面订阅后持久化；启动时由控制面用 [`ProviderHealthSeed`] 全量回灌。
#[derive(Debug)]
pub struct HealthManager {
    states: RwLock<HashMap<String, HealthState>>,
    events: EventBus,
}

impl HealthManager {
    /// 用控制面回灌的种子（含冷却截止时刻）建立状态机。
    pub fn restore(seeds: Vec<ProviderHealthSeed>, events: EventBus) -> Arc<Self> {
        let states = seeds
            .into_iter()
            .map(|seed| (seed.provider_id, seed.state))
            .collect();
        Arc::new(Self {
            states: RwLock::new(states),
            events,
        })
    }

    /// 当前状态（未知 provider 视为健康）。
    pub async fn state(&self, provider: &str) -> HealthState {
        self.states
            .read()
            .await
            .get(provider)
            .cloned()
            .unwrap_or(HealthState::Healthy)
    }

    /// provider 是否已永久退役。
    pub async fn is_retired(&self, provider: &str) -> bool {
        matches!(self.state(provider).await, HealthState::Retired { .. })
    }

    /// 冷却是否已到期（到期后需先探针）。
    pub async fn is_cooldown_expired(&self, provider: &str, now: i64) -> bool {
        match self.state(provider).await {
            HealthState::Quarantined { until, .. } => until <= now,
            _ => false,
        }
    }

    /// 应用一次标记：`Retire` 不可被冷却覆盖；更晚的冷却覆盖更早的。发生迁移时发事件。
    pub async fn apply(&self, provider: &str, mark: Mark, reason: MarkReason, now: i64) {
        let decided = {
            let mut states = self.states.write().await;
            let current = states.get(provider).cloned().unwrap_or(HealthState::Healthy);
            let next = match mark {
                Mark::None => None,
                Mark::Retire => match current {
                    HealthState::Retired { .. } => None,
                    _ => Some(HealthState::Retired {
                        reason: reason.as_str().to_string(),
                    }),
                },
                Mark::Quarantine(duration) => {
                    let until = now.saturating_add(duration.as_secs() as i64);
                    match &current {
                        HealthState::Retired { .. } => None,
                        HealthState::Quarantined {
                            until: existing, ..
                        } if *existing >= until => None,
                        _ => Some(HealthState::Quarantined {
                            until,
                            reason: reason.as_str().to_string(),
                        }),
                    }
                }
            };
            if let Some(state) = &next {
                states.insert(provider.to_string(), state.clone());
            }
            next
        };

        if let Some(state) = decided {
            tracing::warn!(provider = %provider, reason = %reason.as_str(), "provider marked unavailable");
            self.events.emit(CoreEvent::HealthChanged(HealthChangedEvent {
                provider_id: provider.to_string(),
                state,
                at: now,
            }));
        }
    }

    /// 探针通过：清除标记回池并发事件。
    pub async fn mark_healthy(&self, provider: &str, now: i64) {
        let removed = self.states.write().await.remove(provider);
        if removed.is_some() {
            tracing::info!(provider = %provider, "probe passed; provider back in pool");
            self.events.emit(CoreEvent::HealthChanged(HealthChangedEvent {
                provider_id: provider.to_string(),
                state: HealthState::Healthy,
                at: now,
            }));
        }
    }

    /// 测试用：把冷却截止时刻拨到过去，模拟时间流逝。
    #[cfg(test)]
    pub(crate) async fn force_cooldown_expired(&self, provider: &str) {
        let mut states = self.states.write().await;
        if let Some(HealthState::Quarantined { until, .. }) = states.get_mut(provider) {
            *until = 0;
        }
    }
}

/// 探测中的 provider 集合：保证同一 provider 同时最多一个探针。
#[derive(Debug, Default)]
pub struct Probing {
    in_flight: Mutex<HashSet<String>>,
}

impl Probing {
    /// 标记 provider 进入探测；若已有探针在飞则返回 `None`。
    pub async fn begin(&self, provider: &str) -> Option<()> {
        let mut in_flight = self.in_flight.lock().await;
        if in_flight.contains(provider) {
            return None;
        }
        in_flight.insert(provider.to_string());
        Some(())
    }

    /// 探针结束，释放占位。
    pub async fn end(&self, provider: &str) {
        self.in_flight.lock().await.remove(provider);
    }

    /// provider 是否正在探测（探测中不参与选号）。
    pub async fn is_in_flight(&self, provider: &str) -> bool {
        self.in_flight.lock().await.contains(provider)
    }
}

/// 维护 [`Probing`] 占位的守卫：探针任务无论何种退出路径都会释放。
pub struct ProbeGuard {
    probing: Arc<Probing>,
    provider: Option<String>,
}

impl Drop for ProbeGuard {
    fn drop(&mut self) {
        let Some(provider) = self.provider.take() else {
            return;
        };
        let probing = Arc::clone(&self.probing);
        tokio::spawn(async move {
            probing.end(&provider).await;
        });
    }
}

/// 开始探测：返回 `None` 表示该 provider 已有探针在飞。
pub async fn begin_probe(probing: &Arc<Probing>, provider: &str) -> Option<ProbeGuard> {
    probing.begin(provider).await?;
    Some(ProbeGuard {
        probing: Arc::clone(probing),
        provider: Some(provider.to_string()),
    })
}
