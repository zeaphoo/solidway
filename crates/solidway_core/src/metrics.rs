//! 选号指标：每个 provider 的首字节时延与失败率（内存 EWMA）。
//!
//! 硬约束：只在内存里，不落库、不读库（`solidway_core` 的纯内存边界）。
//! 重启清零；距最后一次样本超过 [`STALE_SECS`] 视为未知，由打分回落到阈值兜底值。

use std::collections::HashMap;
use std::sync::RwLock;

/// EWMA 平滑系数。
const ALPHA: f64 = 0.2;
/// 陈旧判定：30 分钟。
pub const STALE_SECS: i64 = 30 * 60;

#[derive(Debug, Clone, Copy, Default)]
struct Stats {
    ttfb_ms: f64,
    failure_rate: f64,
    samples: u64,
    last_sample_at: i64,
}

/// 打分用的一次取值（只包含新鲜样本）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricValue {
    pub ttfb_ms: f64,
    pub uptime_pct: f64,
}

/// 展示用的指标快照（含陈旧标记，无样本时数值为空）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatsView {
    pub ttfb_ms: Option<f64>,
    pub uptime_pct: Option<f64>,
    pub samples: u64,
    pub stale: bool,
}

impl StatsView {
    pub fn missing() -> Self {
        Self {
            ttfb_ms: None,
            uptime_pct: None,
            samples: 0,
            stale: true,
        }
    }
}

/// 所有 provider 的选号指标。
#[derive(Debug, Default)]
pub struct Metrics {
    inner: RwLock<HashMap<String, Stats>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// 成功拿到上游响应头：更新时延与失败率（向 0 收敛）。
    pub fn record_success(&self, provider_id: &str, ttfb_ms: u64, now: i64) {
        let mut guard = self.inner.write().expect("metrics lock");
        let stats = guard.entry(provider_id.to_string()).or_default();
        let sample = ttfb_ms as f64;
        stats.ttfb_ms = if stats.samples == 0 {
            sample
        } else {
            (1.0 - ALPHA) * stats.ttfb_ms + ALPHA * sample
        };
        stats.failure_rate -= ALPHA * stats.failure_rate;
        stats.samples += 1;
        stats.last_sample_at = now;
    }

    /// 标记类失败 / 传输失败：失败率向 1 收敛，时延不更新。
    pub fn record_failure(&self, provider_id: &str, now: i64) {
        let mut guard = self.inner.write().expect("metrics lock");
        let stats = guard.entry(provider_id.to_string()).or_default();
        stats.failure_rate += ALPHA * (1.0 - stats.failure_rate);
        stats.samples += 1;
        stats.last_sample_at = now;
    }

    /// 启动种子：只在本地无样本时采用；`last_sample_at` 取当前时刻，
    /// 否则按整点对齐的历史均值会被立刻判定为陈旧。
    ///
    /// 返回是否采用。
    pub fn seed(&self, provider_id: &str, ttfb_ms: f64, failure_rate: f64, samples: u64, now: i64) -> bool {
        if !ttfb_ms.is_finite() || ttfb_ms < 0.0 || samples == 0 {
            return false;
        }
        let mut guard = self.inner.write().expect("metrics lock");
        if guard.get(provider_id).is_some_and(|stats| stats.samples > 0) {
            return false;
        }
        guard.insert(
            provider_id.to_string(),
            Stats {
                ttfb_ms,
                failure_rate: failure_rate.clamp(0.0, 1.0),
                samples,
                last_sample_at: now,
            },
        );
        true
    }

    /// 某 provider 的样本数（种子采用判定用）。
    pub fn samples(&self, provider_id: &str) -> u64 {
        self.inner
            .read()
            .expect("metrics lock")
            .get(provider_id)
            .map(|stats| stats.samples)
            .unwrap_or(0)
    }

    /// 打分取值：无样本或陈旧时返回 `None`。
    pub fn value(&self, provider_id: &str, now: i64) -> Option<MetricValue> {
        let guard = self.inner.read().expect("metrics lock");
        let stats = guard.get(provider_id)?;
        if stats.samples == 0 || now.saturating_sub(stats.last_sample_at) > STALE_SECS {
            return None;
        }
        Some(MetricValue {
            ttfb_ms: stats.ttfb_ms,
            uptime_pct: (1.0 - stats.failure_rate) * 100.0,
        })
    }

    /// 展示取值：陈旧也返回数值，只是标记 `stale`。
    pub fn view(&self, provider_id: &str, now: i64) -> StatsView {
        let guard = self.inner.read().expect("metrics lock");
        let Some(stats) = guard.get(provider_id) else {
            return StatsView::missing();
        };
        StatsView {
            ttfb_ms: Some(stats.ttfb_ms),
            uptime_pct: Some((1.0 - stats.failure_rate) * 100.0),
            samples: stats.samples,
            stale: now.saturating_sub(stats.last_sample_at) > STALE_SECS,
        }
    }

    /// 丢弃不再存在的 provider 的指标。
    pub fn retain(&self, keep: impl Fn(&str) -> bool) {
        let mut guard = self.inner.write().expect("metrics lock");
        guard.retain(|id, _| keep(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_updates_ttfb_and_failure_rate() {
        let metrics = Metrics::new();
        metrics.record_success("p1", 200, 1000);
        let value = metrics.value("p1", 1000).expect("value");
        assert!((value.ttfb_ms - 200.0).abs() < f64::EPSILON);
        assert!((value.uptime_pct - 100.0).abs() < f64::EPSILON);

        metrics.record_success("p1", 400, 1001);
        let value = metrics.value("p1", 1001).expect("value");
        // EWMA：0.8×200 + 0.2×400 = 240
        assert!((value.ttfb_ms - 240.0).abs() < 1e-9);
    }

    #[test]
    fn failure_lowers_uptime_without_touching_ttfb() {
        let metrics = Metrics::new();
        metrics.record_success("p1", 300, 1000);
        metrics.record_failure("p1", 1001);
        let value = metrics.value("p1", 1001).expect("value");
        assert!((value.ttfb_ms - 300.0).abs() < f64::EPSILON);
        // 失败率 = 0.8×0 + 0.2 = 0.2 → uptime 80%
        assert!((value.uptime_pct - 80.0).abs() < 1e-9);
    }

    #[test]
    fn stale_samples_fall_back_to_defaults() {
        let metrics = Metrics::new();
        metrics.record_success("p1", 300, 1000);
        assert!(metrics.value("p1", 1000 + STALE_SECS).is_some());
        assert!(metrics.value("p1", 1000 + STALE_SECS + 1).is_none());

        let view = metrics.view("p1", 1000 + STALE_SECS + 1);
        assert!(view.stale);
        assert!(view.ttfb_ms.is_some());
        assert_eq!(view.samples, 1);
    }

    #[test]
    fn missing_provider_has_no_value() {
        let metrics = Metrics::new();
        assert!(metrics.value("nope", 1000).is_none());
        assert_eq!(metrics.view("nope", 1000), StatsView::missing());
    }

    #[test]
    fn seed_applies_only_without_samples() {
        let metrics = Metrics::new();
        assert!(metrics.seed("p1", 250.0, 0.1, 40, 1000));
        let view = metrics.view("p1", 1000);
        assert_eq!(view.samples, 40);
        assert!((view.ttfb_ms.unwrap() - 250.0).abs() < f64::EPSILON);
        assert!(!view.stale, "种子以下发时刻为样本时间，不应立刻陈旧");

        // 已有样本后不再采用种子。
        assert!(!metrics.seed("p1", 900.0, 0.9, 999, 1001));
        assert_eq!(metrics.samples("p1"), 40);
    }

    #[test]
    fn seed_rejects_invalid_values() {
        let metrics = Metrics::new();
        assert!(!metrics.seed("p1", f64::NAN, 0.0, 1, 1));
        assert!(!metrics.seed("p1", -1.0, 0.0, 1, 1));
        assert!(!metrics.seed("p1", 100.0, 0.0, 0, 1));
        assert_eq!(metrics.samples("p1"), 0);
    }

    #[test]
    fn retain_drops_removed_providers() {
        let metrics = Metrics::new();
        metrics.record_success("p1", 100, 1000);
        metrics.record_success("p2", 100, 1000);
        metrics.retain(|id| id == "p1");
        assert!(metrics.value("p1", 1000).is_some());
        assert!(metrics.value("p2", 1000).is_none());
    }
}
