//! 路由选号策略的设置层：默认值表、覆盖项合并与校验。
//!
//! 存储只放「用户显式改过的字段」（`selection_config`），解析时与内置默认值合并，
//! 因此新增参数不需要迁移。

use serde::{Deserialize, Serialize};
use solidway_core::{
    RouteSelection, SelectionStrategy, SelectionThresholds, SelectionWeights,
};

use crate::error::{ApiError, Result};

/// `selection_config` 的存储形状：全部字段可选。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SelectionOverrides {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weights: Option<WeightsOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thresholds: Option<ThresholdsOverride>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeightsOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThresholdsOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_uptime: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_latency_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exploration_rate: Option<f64>,
}

/// 解析后的完整配置（下发给 core 的形态）。
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedSelection {
    pub strategy: SelectionStrategy,
    pub weights: SelectionWeights,
    pub thresholds: SelectionThresholds,
    /// 权重和为 0 → 数据面回退轮询。
    pub fallback: bool,
}

/// 解析：默认值 ∪ 覆盖项。
pub fn resolve(strategy: SelectionStrategy, overrides: &SelectionOverrides) -> ResolvedSelection {
    let base = RouteSelection::for_strategy(strategy);
    let mut weights = base.weights;
    if let Some(patch) = &overrides.weights {
        weights.uptime = patch.uptime.unwrap_or(weights.uptime);
        weights.latency = patch.latency.unwrap_or(weights.latency);
        weights.price = patch.price.unwrap_or(weights.price);
        weights.throughput = patch.throughput.unwrap_or(weights.throughput);
    }
    let mut thresholds = base.thresholds;
    if let Some(patch) = &overrides.thresholds {
        thresholds.uptime_penalty = patch.uptime_penalty.unwrap_or(thresholds.uptime_penalty);
        thresholds.default_uptime = patch.default_uptime.unwrap_or(thresholds.default_uptime);
        thresholds.default_latency_ms = patch
            .default_latency_ms
            .unwrap_or(thresholds.default_latency_ms);
        thresholds.exploration_rate = patch
            .exploration_rate
            .unwrap_or(thresholds.exploration_rate);
    }
    let selection = RouteSelection {
        strategy,
        weights,
        thresholds,
    };
    ResolvedSelection {
        strategy,
        weights,
        thresholds,
        fallback: selection.falls_back(),
    }
}

/// 校验覆盖项；返回 `SelectionOverrides` 以便直接落库。
pub fn validate(overrides: &SelectionOverrides) -> Result<()> {
    if let Some(patch) = &overrides.weights {
        for (name, value) in [("uptime", patch.uptime), ("latency", patch.latency)] {
            if let Some(value) = value
                && !(0.0..=1.0).contains(&value)
            {
                return Err(ApiError::InvalidRequest(format!(
                    "weights.{name} 必须在 0-1 之间"
                )));
            }
        }
        for (name, value) in [("price", patch.price), ("throughput", patch.throughput)] {
            if let Some(value) = value
                && value != 0.0
            {
                return Err(ApiError::InvalidRequest(format!(
                    "weights.{name} 本轮必须为 0（暂无对应指标）"
                )));
            }
        }
    }
    if let Some(patch) = &overrides.thresholds {
        if let Some(value) = patch.uptime_penalty
            && !(1.0..=100.0).contains(&value)
        {
            return Err(ApiError::InvalidRequest(
                "thresholds.uptime_penalty 必须在 1-100 之间".to_string(),
            ));
        }
        if let Some(value) = patch.default_uptime
            && !(value > 0.0 && value <= 100.0)
        {
            return Err(ApiError::InvalidRequest(
                "thresholds.default_uptime 必须在 (0, 100] 之间".to_string(),
            ));
        }
        if let Some(value) = patch.default_latency_ms
            && !(1.0..=600_000.0).contains(&value)
        {
            return Err(ApiError::InvalidRequest(
                "thresholds.default_latency_ms 必须在 1-600000 之间".to_string(),
            ));
        }
        if let Some(value) = patch.exploration_rate
            && !(0.0..=1.0).contains(&value)
        {
            return Err(ApiError::InvalidRequest(
                "thresholds.exploration_rate 必须在 0-1 之间".to_string(),
            ));
        }
    }
    Ok(())
}

/// 从数据库行解析（策略列 + JSON 列）。解析失败时回退默认值并记录告警。
pub fn from_row(strategy: &str, config: &str) -> (SelectionStrategy, SelectionOverrides) {
    let strategy = SelectionStrategy::parse(strategy).unwrap_or_default();
    let overrides = if config.trim().is_empty() {
        SelectionOverrides::default()
    } else {
        serde_json::from_str::<SelectionOverrides>(config).unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to parse selection_config; using defaults");
            SelectionOverrides::default()
        })
    };
    (strategy, overrides)
}

/// 落库前的序列化（`{}` 表示无覆盖）。
pub fn to_json(overrides: &SelectionOverrides) -> String {
    serde_json::to_string(overrides).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_defaults_are_per_strategy() {
        let latency = resolve(SelectionStrategy::Latency, &SelectionOverrides::default());
        assert_eq!(latency.weights.latency, 0.9);
        assert_eq!(latency.weights.uptime, 0.1);
        assert_eq!(latency.thresholds.exploration_rate, 0.0);

        let auto = resolve(SelectionStrategy::Auto, &SelectionOverrides::default());
        assert_eq!(auto.weights.uptime, 0.75);
        assert_eq!(auto.thresholds.exploration_rate, 0.01);
        assert!(!auto.fallback);
    }

    #[test]
    fn overrides_merge_on_top_of_defaults() {
        let overrides = SelectionOverrides {
            weights: Some(WeightsOverride {
                uptime: Some(0.8),
                ..Default::default()
            }),
            thresholds: Some(ThresholdsOverride {
                uptime_penalty: Some(90.0),
                ..Default::default()
            }),
        };
        let resolved = resolve(SelectionStrategy::Auto, &overrides);
        assert_eq!(resolved.weights.uptime, 0.8);
        assert_eq!(resolved.weights.latency, 0.25);
        assert_eq!(resolved.thresholds.uptime_penalty, 90.0);
    }

    #[test]
    fn zero_weights_mark_fallback() {
        let overrides = SelectionOverrides {
            weights: Some(WeightsOverride {
                uptime: Some(0.0),
                latency: Some(0.0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let resolved = resolve(SelectionStrategy::Auto, &overrides);
        assert!(resolved.fallback);
        assert_eq!(
            RouteSelection {
                strategy: resolved.strategy,
                weights: resolved.weights,
                thresholds: resolved.thresholds,
            }
            .effective_strategy(),
            SelectionStrategy::RoundRobin
        );
    }

    #[test]
    fn rejects_price_and_throughput_weights() {
        let overrides = SelectionOverrides {
            weights: Some(WeightsOverride {
                price: Some(0.3),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(validate(&overrides).is_err());
    }

    #[test]
    fn rejects_out_of_range_thresholds() {
        for patch in [
            ThresholdsOverride {
                uptime_penalty: Some(0.0),
                ..Default::default()
            },
            ThresholdsOverride {
                default_uptime: Some(0.0),
                ..Default::default()
            },
            ThresholdsOverride {
                default_latency_ms: Some(0.0),
                ..Default::default()
            },
            ThresholdsOverride {
                exploration_rate: Some(1.5),
                ..Default::default()
            },
        ] {
            let overrides = SelectionOverrides {
                thresholds: Some(patch),
                ..Default::default()
            };
            assert!(validate(&overrides).is_err());
        }
    }

    #[test]
    fn malformed_config_falls_back_to_defaults() {
        let (strategy, overrides) = from_row("latency", "not json");
        assert_eq!(strategy, SelectionStrategy::Latency);
        assert!(overrides.weights.is_none());
    }

    #[test]
    fn unknown_strategy_falls_back_to_round_robin() {
        let (strategy, _) = from_row("nope", "{}");
        assert_eq!(strategy, SelectionStrategy::RoundRobin);
    }
}
