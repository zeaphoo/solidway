//! 运行期注册表：由 [`CoreConfig`] 构建的路由 / provider 索引与选号器（纯内存）。
//!
//! 路由为**两级索引**：`key_hash → (api_key, route)`；路由内按模型建候选索引（R1-F-12② 术语对齐）
//! `model → [provider]`（同一路由内提供同名模型的 provider 合并为一组）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use crate::config::{
    ApiFormat, CoreConfig, ModelMapping, ProviderEntry, RouteSelection, SelectionStrategy,
};
use crate::health::{HealthManager, HealthState, Probing, epoch_secs};
use crate::metrics::Metrics;

/// 运行期的一个 provider 凭据。
#[derive(Debug, Clone)]
pub struct ProviderRuntime {
    pub id: String,
    pub name: Option<String>,
    pub provider: String,
    pub base_url: String,
    pub api_key: String,
    pub credential_kind: String,
    pub auth_scheme: String,
    /// Provider 级非敏感选项（原样透传给转发层）。
    pub options: Option<serde_json::Value>,
    pub api_formats: Vec<ApiFormat>,
    pub models: Vec<ModelMapping>,
    pub enabled: bool,
    /// 选号档位：`0` = 不参与选号。
    pub priority: i64,
}

impl ProviderRuntime {
    /// 探针模型名：清单首项的上游 id；清单为空（不服务任何 model）时为 `None`。
    pub fn probe_model(&self) -> Option<&str> {
        self.models.first().map(ModelMapping::upstream_or_name)
    }

    /// 是否声明了上游路径模板（决定目标地址构造方式与资源端点可用性）。
    pub fn is_templated(&self) -> bool {
        self.options
            .as_ref()
            .and_then(|value| value.get("path_template"))
            .and_then(|value| value.as_str())
            .is_some_and(|template| !template.trim().is_empty())
    }

    /// 该凭据是否声明了此客户端模型名。
    pub fn serves_model(&self, name: &str) -> bool {
        self.models.iter().any(|mapping| mapping.name == name)
    }

    /// 客户端模型名对应的上游模型 id。
    pub fn upstream_model(&self, name: &str) -> Option<&str> {
        self.models
            .iter()
            .find(|mapping| mapping.name == name)
            .map(ModelMapping::upstream_or_name)
    }

    /// 探针使用的入站格式，按固定优先级：chat completions → responses → anthropic messages。
    ///
    /// 空 `api_formats` 表示没有可服务的入站格式，因此不会被探测。
    pub fn probe_format(&self) -> Option<ApiFormat> {
        [
            ApiFormat::ChatCompletions,
            ApiFormat::Responses,
            ApiFormat::AnthropicMessages,
        ]
        .into_iter()
        .find(|format| self.api_formats.contains(format))
    }
}

/// 运行期的一个路由。
#[derive(Debug)]
pub struct RouteRuntime {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub providers: Vec<ProviderRuntime>,
    /// 模型并集（字典序），供 `GET /v1/models` 本地合成。
    models_union: Vec<String>,
    /// 任意 Provider（含已禁用）声明过的模型，用于区分「模型不存在」与「格式不支持」。
    models_set: HashSet<String>,
    /// 任意 Provider（含已禁用）声明过的 `(模型, 格式)`，用于能力预检。
    capability: HashSet<(String, ApiFormat)>,
    /// (模型, 格式) → 候选 provider 序号（仅已启用；同组合并为同一候选集合）。
    rotation: HashMap<(String, ApiFormat), Vec<usize>>,
    /// 模型 → 成员 provider 序号（不限格式，供探针挑选）。
    model_members: HashMap<String, Vec<usize>>,
    /// 选号策略（控制面解析后的完整配置）。
    pub selection: RouteSelection,
}

impl RouteRuntime {
    pub fn models(&self) -> &[String] {
        &self.models_union
    }

    /// 该模型是否被任意 Provider 声明（含已禁用）。
    pub fn has_model(&self, model: &str) -> bool {
        self.models_set.contains(model)
    }

    /// 是否存在 Provider 声明了该 `(模型, 格式)` 组合（含已禁用）。
    pub fn supports_format(&self, model: &str, format: ApiFormat) -> bool {
        self.capability.contains(&(model.to_string(), format))
    }

    /// 该 `(模型, 格式)` 的候选 provider 序号（按配置顺序，仅已启用）。
    pub fn rotation_group(&self, model: &str, format: ApiFormat) -> &[usize] {
        self.rotation
            .get(&(model.to_string(), format))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn provider(&self, index: usize) -> &ProviderRuntime {
        &self.providers[index]
    }
}

/// API Key 的解析结果：`key_hash` → 所属路由。
#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub api_key_id: String,
    pub route_id: String,
    pub expires_at: Option<i64>,
}

/// 由配置构建的不可变注册表。
#[derive(Debug, Default)]
pub struct Registry {
    routes: HashMap<String, RouteRuntime>,
    by_key_hash: HashMap<String, KeyBinding>,
    providers: HashMap<String, ProviderRuntime>,
}

fn to_runtime(entry: &ProviderEntry) -> ProviderRuntime {
    ProviderRuntime {
        id: entry.id.clone(),
        name: entry.name.clone(),
        provider: entry.provider.clone(),
        base_url: entry.base_url.clone(),
        api_key: entry.api_key.clone(),
        credential_kind: entry.credential_kind.clone(),
        auth_scheme: entry.auth_scheme.clone(),
        options: entry.options.clone(),
        api_formats: entry.api_formats.clone(),
        models: entry.models.clone(),
        enabled: entry.enabled,
        priority: entry.priority,
    }
}

impl Registry {
    pub fn new(config: &CoreConfig) -> Self {
        let mut routes = HashMap::with_capacity(config.routes.len());
        let mut providers = HashMap::with_capacity(config.providers.len());

        for entry in &config.providers {
            providers.insert(entry.id.clone(), to_runtime(entry));
        }

        for route in &config.routes {
            let members: Vec<ProviderRuntime> = config
                .providers
                .iter()
                .filter(|p| p.route_id == route.id)
                .map(to_runtime)
                .collect();

            // 候选索引：仅已启用 provider；每 (模型, 格式) 至多入组一次；空 `models` 不入组。
            let mut rotation: HashMap<(String, ApiFormat), Vec<usize>> = HashMap::new();
            let mut models_set: HashSet<String> = HashSet::new();
            let mut inbound_models: HashSet<String> = HashSet::new();
            let mut capability: HashSet<(String, ApiFormat)> = HashSet::new();
            let mut model_members: HashMap<String, Vec<usize>> = HashMap::new();
            for (index, provider) in members.iter().enumerate() {
                if !provider.enabled {
                    continue;
                }
                // 防御：合法档位是 1..=1000（不参与选号请停用 Provider）。
                // 非法档位（≤0）不参与选号，但仍保留模型与能力声明，
                // 这样外层能区分「模型不存在」与「无可用凭据」。
                let selectable = provider.priority > 0;
                let mut seen: HashSet<(&str, ApiFormat)> = HashSet::new();
                for mapping in &provider.models {
                    let model = &mapping.name;
                    models_set.insert(model.clone());
                    model_members.entry(model.clone()).or_default().push(index);
                    for format in &provider.api_formats {
                        capability.insert((model.clone(), *format));
                        inbound_models.insert(model.clone());
                        if selectable && seen.insert((model.as_str(), *format)) {
                            rotation
                                .entry((model.clone(), *format))
                                .or_default()
                                .push(index);
                        }
                    }
                }
            }
            // `/v1/models` 只暴露至少有一种入站格式可服务的模型；`models_set` 保留完整集合，
            // 用于区分「模型不存在」与「格式不支持」。
            let mut models_union: Vec<String> = inbound_models.into_iter().collect();
            models_union.sort();

            routes.insert(
                route.id.clone(),
                RouteRuntime {
                    id: route.id.clone(),
                    name: route.name.clone(),
                    enabled: route.enabled,
                    providers: members,
                    models_union,
                    models_set,
                    capability,
                    rotation,
                    model_members,
                    selection: route.selection,
                },
            );
        }

        let mut by_key_hash = HashMap::new();
        for key in &config.api_keys {
            if !key.enabled {
                continue;
            }
            by_key_hash.insert(
                key.key_hash.clone(),
                KeyBinding {
                    api_key_id: key.id.clone(),
                    route_id: key.route_id.clone(),
                    expires_at: key.expires_at,
                },
            );
        }

        Self {
            routes,
            by_key_hash,
            providers,
        }
    }

    /// 按 API Key 哈希定位（key → route）。
    pub fn key_binding(&self, key_hash: &str) -> Option<&KeyBinding> {
        self.by_key_hash.get(key_hash)
    }

    pub fn route(&self, id: &str) -> Option<&RouteRuntime> {
        self.routes.get(id)
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderRuntime> {
        self.providers.get(id)
    }

    pub fn routes(&self) -> impl Iterator<Item = &RouteRuntime> {
        self.routes.values()
    }
}

/// 选号结果（成员为 provider 凭据 id）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    /// 粘性命中的首选 provider（健康时）。
    pub preferred: Option<String>,
    /// 其余可用 provider，按该 (路由, 模型) 计数器的轮询顺序。
    pub candidates: Vec<String>,
}

impl Pick {
    /// 本轮尝试顺序：粘性优先，其余按轮询顺序。
    pub fn order(&self) -> Vec<String> {
        let mut order = Vec::with_capacity(self.candidates.len() + 1);
        if let Some(preferred) = &self.preferred {
            order.push(preferred.clone());
        }
        order.extend(self.candidates.iter().cloned());
        order
    }
}

/// 选号器：每 (路由, 模型) 一个轮询计数器 + 健康过滤 + 策略排序 + 惰性探测目标。
#[derive(Debug)]
pub struct Selector {
    counters: HashMap<(String, String), AtomicUsize>,
    health: Arc<HealthManager>,
    metrics: Arc<Metrics>,
}

impl Selector {
    pub fn new(registry: &Registry, health: Arc<HealthManager>, metrics: Arc<Metrics>) -> Self {
        let mut counters: HashMap<(String, String), AtomicUsize> = HashMap::new();
        for route in registry.routes() {
            for model in route.models() {
                counters
                    .entry((route.id.clone(), model.clone()))
                    .or_insert_with(|| AtomicUsize::new(0));
            }
        }
        Self {
            counters,
            health,
            metrics,
        }
    }

    /// 选号：候选 = 提供该模型与格式、健康、`priority > 0` 的 provider；
    /// 顺序由路由的选号策略决定（默认轮询，与引入策略之前一致）。
    pub async fn pick(
        &self,
        route: &RouteRuntime,
        model: &str,
        format: ApiFormat,
        affinity: Option<&str>,
        probing: &Arc<Probing>,
    ) -> Pick {
        let group = route.rotation_group(model, format);
        let mut usable: Vec<usize> = Vec::with_capacity(group.len());
        for &index in group {
            let provider = route.provider(index);
            if !matches!(self.health.state(&provider.id).await, HealthState::Healthy) {
                continue;
            }
            if probing.is_in_flight(&provider.id).await {
                continue;
            }
            usable.push(index);
        }

        let mut preferred = None;
        if let Some(id) = affinity
            && usable.iter().any(|&index| route.provider(index).id == id)
        {
            preferred = Some(id.to_string());
        }

        let counter = self
            .counters
            .get(&(route.id.clone(), model.to_string()))
            .map(|counter| counter.fetch_add(1, Ordering::Relaxed))
            .unwrap_or(0);

        let ordered = self.order_candidates(route, &usable, counter);
        let mut candidates = Vec::with_capacity(ordered.len());
        for id in ordered {
            if preferred.as_deref() == Some(id.as_str()) {
                continue;
            }
            candidates.push(id);
        }

        Pick {
            preferred,
            candidates,
        }
    }

    /// 按策略把候选排成尝试顺序（不含粘性偏好）。
    ///
    /// 排序键是 **(priority 降序, 策略顺序)**：先按档位分组，档内再按策略
    /// （轮询 / 打分）排列，档内同分或轮询起点用计数器打散。
    fn order_candidates(
        &self,
        route: &RouteRuntime,
        usable: &[usize],
        counter: usize,
    ) -> Vec<String> {
        if usable.is_empty() {
            return Vec::new();
        }
        let selection = route.selection;
        let strategy = selection.effective_strategy();
        let explore = strategy == SelectionStrategy::Auto
            && selection.thresholds.exploration_rate > 0.0
            && random_unit() < selection.thresholds.exploration_rate;
        let now = epoch_secs(SystemTime::now());
        let scores = score_map(route, usable, &selection, &self.metrics, now);

        let len = usable.len();
        let start = counter % len;
        // 平局打散：与轮询计数器同序。
        let rank = |offset: usize| (offset + len - start) % len;

        // 档位分组（R1-F-6）：先按 priority 稳定排序，再把**非相邻的同档**合并为同档，
        // 避免输入形如 [5,1,5] 时只合并相邻项、导致同档被拆成多组。
        let mut by_priority: Vec<usize> = (0..len).collect();
        by_priority
            .sort_by_key(|&offset| std::cmp::Reverse(route.provider(usable[offset]).priority));
        let mut tiers: Vec<(i64, Vec<usize>)> = Vec::new();
        for offset in by_priority {
            let priority = route.provider(usable[offset]).priority;
            match tiers.last_mut() {
                Some((tier, offsets)) if *tier == priority => offsets.push(offset),
                _ => tiers.push((priority, vec![offset])),
            }
        }

        let mut ordered = Vec::with_capacity(len);
        for (_priority, mut offsets) in tiers {
            if strategy == SelectionStrategy::RoundRobin || explore {
                offsets.sort_by_key(|&offset| rank(offset));
            } else {
                offsets.sort_by(|&a, &b| {
                    let (sa, sb) = (scores.get(&usable[a]), scores.get(&usable[b]));
                    sa.copied()
                        .unwrap_or(f64::MAX)
                        .partial_cmp(&sb.copied().unwrap_or(f64::MAX))
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(rank(a).cmp(&rank(b)))
                });
            }
            ordered.extend(
                offsets
                    .into_iter()
                    .map(|offset| route.provider(usable[offset]).id.clone()),
            );
        }
        ordered
    }

    /// 只读快照：某路由（可选模型）下每个 Provider 的指标与当前分数。
    pub async fn snapshot(
        &self,
        route: &RouteRuntime,
        model: Option<&str>,
    ) -> Vec<ProviderSnapshot> {
        let now = epoch_secs(SystemTime::now());
        let model = model
            .map(str::to_string)
            .or_else(|| route.models().first().cloned());
        let selection = route.selection;
        let strategy = selection.effective_strategy();

        let members: Vec<usize> = match &model {
            Some(model) => (0..route.providers.len())
                .filter(|&index| {
                    let provider = route.provider(index);
                    provider.enabled && provider.priority > 0 && provider.serves_model(model)
                })
                .collect(),
            None => Vec::new(),
        };
        let scores = score_map(route, &members, &selection, &self.metrics, now);

        let mut out = Vec::with_capacity(route.providers.len());
        for (index, provider) in route.providers.iter().enumerate() {
            let view = self.metrics.view(&provider.id, now);
            let health = self.health.state(&provider.id).await;
            out.push(ProviderSnapshot {
                provider_id: provider.id.clone(),
                name: provider.name.clone(),
                priority: provider.priority,
                enabled: provider.enabled,
                health: health.as_str().to_string(),
                ttfb_ms: view.ttfb_ms,
                uptime_pct: view.uptime_pct,
                samples: view.samples,
                stale: view.stale,
                score: match strategy {
                    SelectionStrategy::RoundRobin => None,
                    _ => scores.get(&index).copied(),
                },
            });
        }
        out
    }
}

/// 某路由下单个 Provider 的选号快照（只读展示用）。
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderSnapshot {
    pub provider_id: String,
    pub name: Option<String>,
    pub priority: i64,
    pub enabled: bool,
    /// `healthy` / `quarantined` / `retired`。
    pub health: String,
    pub ttfb_ms: Option<f64>,
    pub uptime_pct: Option<f64>,
    pub samples: u64,
    pub stale: bool,
    /// 当前策略下的分数；轮询策略为 `None`。
    pub score: Option<f64>,
}

/// 候选集合的打分（越小越好），`route_provider_index → score`。
///
/// `score = (w_uptime/Σw)·uptimeRatio + (w_latency/Σw)·latencyRatio + uptimePenalty`
///
/// `priority` 不进分数：它是**档位**，由 [`Selector::order_candidates`] 先分组。
fn score_map(
    route: &RouteRuntime,
    members: &[usize],
    selection: &RouteSelection,
    metrics: &Metrics,
    now: i64,
) -> HashMap<usize, f64> {
    let mut out = HashMap::with_capacity(members.len());
    if members.is_empty() || selection.effective_strategy() == SelectionStrategy::RoundRobin {
        return out;
    }
    let weights = selection.weights;
    let total = weights.sum();
    if total <= 0.0 {
        return out;
    }
    let thresholds = selection.thresholds;

    let samples: Vec<(usize, f64, f64)> = members
        .iter()
        .map(|&index| {
            let provider = route.provider(index);
            let value = metrics.value(&provider.id, now);
            (
                index,
                value
                    .map(|v| v.ttfb_ms)
                    .unwrap_or(thresholds.default_latency_ms),
                value
                    .map(|v| v.uptime_pct)
                    .unwrap_or(thresholds.default_uptime),
            )
        })
        .collect();

    let min_ttfb = samples
        .iter()
        .map(|(_, ttfb, _)| *ttfb)
        .fold(f64::INFINITY, f64::min);
    let max_uptime = samples
        .iter()
        .map(|(_, _, uptime)| *uptime)
        .fold(f64::NEG_INFINITY, f64::max);

    for (index, ttfb, uptime) in samples {
        let uptime_ratio = if uptime > 0.0 {
            max_uptime / uptime - 1.0
        } else {
            1.0
        };
        let latency_ratio = if min_ttfb > 0.0 {
            ttfb / min_ttfb - 1.0
        } else {
            0.0
        };
        let penalty = if uptime < thresholds.uptime_penalty {
            let deficit =
                (thresholds.uptime_penalty - uptime) / thresholds.uptime_penalty.max(f64::EPSILON);
            (deficit * 5.0).powi(2)
        } else {
            0.0
        };
        let score = (weights.uptime / total) * uptime_ratio
            + (weights.latency / total) * latency_ratio
            + penalty;
        out.insert(index, score);
    }
    out
}

/// `[0,1)` 伪随机数（探索用；不打散加密安全，也不引入额外依赖）。
fn random_unit() -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    static SEED: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let mut x = SEED.fetch_add(0x9E37_79B9_7F4A_7C15, AtomicOrdering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    (x % 10_000) as f64 / 10_000.0
}

impl Selector {
    /// 本轮需要探测的 provider：该模型候选集合内、冷却到期且无探针在飞者。
    pub async fn probe_targets(
        &self,
        route: &RouteRuntime,
        model: &str,
        probing: &Arc<Probing>,
    ) -> Vec<String> {
        let now = epoch_secs(SystemTime::now());
        let mut targets = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for &index in route
            .model_members
            .get(model)
            .map(Vec::as_slice)
            .unwrap_or(&[])
        {
            let provider = route.provider(index);
            if provider.probe_format().is_none() {
                continue;
            }
            if !seen.insert(provider.id.as_str()) {
                continue;
            }
            if probing.is_in_flight(&provider.id).await {
                continue;
            }
            if self.health.is_cooldown_expired(&provider.id, now).await {
                targets.push(provider.id.clone());
            }
        }
        targets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiKeyEntry, CoreConfig, ProviderEntry, RouteEntry};
    use crate::event::EventBus;

    fn provider(id: &str, route: &str, models: &[&str], enabled: bool) -> ProviderEntry {
        ProviderEntry {
            id: id.to_string(),
            route_id: route.to_string(),
            name: None,
            provider: "openai".to_string(),
            base_url: "https://up.example.com/v1".to_string(),
            api_key: "sk-x".to_string(),
            credential_kind: "api_key".to_string(),
            auth_scheme: "bearer".to_string(),
            options: None,
            api_formats: vec![ApiFormat::ChatCompletions, ApiFormat::Responses],
            models: models
                .iter()
                .map(|m| ModelMapping {
                    name: m.to_string(),
                    upstream: String::new(),
                })
                .collect(),
            enabled,
            priority: 1,
        }
    }

    fn config() -> CoreConfig {
        CoreConfig {
            api_keys: vec![ApiKeyEntry {
                id: "k1".to_string(),
                route_id: "r1".to_string(),
                key_hash: "hash1".to_string(),
                enabled: true,
                expires_at: None,
            }],
            routes: vec![RouteEntry {
                id: "r1".to_string(),
                name: "main".to_string(),
                enabled: true,
                selection: RouteSelection::default(),
            }],
            providers: vec![
                provider("p-a", "r1", &["gpt-5", "gpt-5-mini"], true),
                provider("p-b", "r1", &["gpt-5", "gpt-5-mini"], true),
                provider("p-c", "r1", &["gpt-5"], true),
                provider("p-empty", "r1", &[], true),
                provider("p-off", "r1", &["gpt-5"], false),
            ],
            health: vec![],
            metrics: vec![],
            affinity: vec![],
            options: Default::default(),
        }
    }

    #[test]
    fn index_route_and_union() {
        let registry = Registry::new(&config());
        let binding = registry.key_binding("hash1").expect("key routes");
        assert_eq!(binding.route_id, "r1");
        let route = registry.route(&binding.route_id).expect("route");
        assert_eq!(
            route.models(),
            ["gpt-5".to_string(), "gpt-5-mini".to_string()]
        );
        assert!(route.has_model("gpt-5"));
        assert!(!route.has_model("gpt-unknown"));
        // 禁用与空清单的 provider 不入候选集合
        assert_eq!(
            route
                .rotation_group("gpt-5", ApiFormat::ChatCompletions)
                .len(),
            3
        );
        assert_eq!(
            route
                .rotation_group("gpt-5-mini", ApiFormat::ChatCompletions)
                .len(),
            2
        );
        assert!(route.supports_format("gpt-5", ApiFormat::Responses));
    }

    #[test]
    fn format_capability_is_independent() {
        let mut config = config();
        config.providers[2].api_formats = vec![ApiFormat::Responses]; // p-c 只支持 responses
        let registry = Registry::new(&config);
        let route = registry.route("r1").expect("route");
        assert_eq!(
            route
                .rotation_group("gpt-5", ApiFormat::ChatCompletions)
                .len(),
            2
        );
        assert_eq!(route.rotation_group("gpt-5", ApiFormat::Responses).len(), 3);
        assert!(route.supports_format("gpt-5-mini", ApiFormat::Responses));
        assert!(!route.supports_format("gpt-5", ApiFormat::AnthropicMessages));
    }

    #[test]
    fn models_union_excludes_providers_without_inbound_format() {
        let mut config = config();
        config.providers[2].api_formats = vec![];
        config.providers[2].models = vec![ModelMapping {
            name: "only-empty".to_string(),
            upstream: String::new(),
        }];
        let registry = Registry::new(&config);
        let route = registry.route("r1").expect("route");
        // 空格式 Provider 的模型不对外暴露，但仍用于区分 model_not_found / unsupported_api_format。
        assert!(!route.models().contains(&"only-empty".to_string()));
        assert!(route.has_model("only-empty"));
        assert!(!route.supports_format("only-empty", ApiFormat::ChatCompletions));
    }

    #[tokio::test]
    async fn same_tier_is_merged_before_dispersing() {
        // R1-F-6：输入档位形如 [5,1,5]，同档必须合并后再档内打散/轮询，
        // 不能因不相邻被拆成两组（否则第二组永远排在第一组之后）。
        let mut config = config();
        config.providers[0].priority = 5; // p-a
        config.providers[1].priority = 1; // p-b
        config.providers[2].priority = 5; // p-c
        let registry = Registry::new(&config);
        let health = HealthManager::restore(vec![], EventBus::new());
        let selector = Selector::new(&registry, Arc::clone(&health), Arc::new(Metrics::new()));
        let probing = Arc::new(Probing::default());
        let route = registry.route("r1").expect("route");

        let first = selector
            .pick(route, "gpt-5", ApiFormat::ChatCompletions, None, &probing)
            .await
            .order();
        assert_eq!(first.len(), 3);
        assert_eq!(
            first[..2].iter().cloned().collect::<HashSet<_>>(),
            HashSet::from(["p-a".to_string(), "p-c".to_string()]),
            "同档 5 必须先于档位 1：{first:?}"
        );
        assert_eq!(first[2], "p-b");

        let second = selector
            .pick(route, "gpt-5", ApiFormat::ChatCompletions, None, &probing)
            .await
            .order();
        assert_eq!(
            (first[0].clone(), second[0].clone()),
            ("p-a".to_string(), "p-c".to_string()),
            "档内轮询必须覆盖合并后的同档成员：{first:?} / {second:?}"
        );
    }

    #[tokio::test]
    async fn round_robin_skips_disabled() {
        let registry = Registry::new(&config());
        let health = HealthManager::restore(vec![], EventBus::new());
        let selector = Selector::new(&registry, Arc::clone(&health), Arc::new(Metrics::new()));
        let probing = Arc::new(Probing::default());
        let route = registry.route("r1").expect("route");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..4 {
            let pick = selector
                .pick(route, "gpt-5", ApiFormat::ChatCompletions, None, &probing)
                .await;
            seen.insert(pick.order()[0].clone());
        }
        assert_eq!(seen.len(), 3, "轮询只覆盖 3 个健康 provider：{seen:?}");
    }

    #[tokio::test]
    async fn affinity_skips_format_mismatch() {
        let mut config = config();
        config.providers[0].api_formats = vec![ApiFormat::Responses]; // p-a 仅 responses
        let registry = Registry::new(&config);
        let health = HealthManager::restore(vec![], EventBus::new());
        let selector = Selector::new(&registry, Arc::clone(&health), Arc::new(Metrics::new()));
        let probing = Arc::new(Probing::default());
        let route = registry.route("r1").expect("route");

        let pick = selector
            .pick(
                route,
                "gpt-5",
                ApiFormat::ChatCompletions,
                Some("p-a"),
                &probing,
            )
            .await;
        assert!(
            pick.preferred.is_none(),
            "粘性 Provider 格式不匹配时不得首选"
        );
        assert!(!pick.order().contains(&"p-a".to_string()));
    }

    #[tokio::test]
    async fn quarantined_excluded() {
        let registry = Registry::new(&config());
        let health = HealthManager::restore(vec![], EventBus::new());
        health
            .apply(
                "p-a",
                crate::health::Mark::Quarantine(std::time::Duration::from_secs(600)),
                crate::health::MarkReason::Http429,
                epoch_secs(SystemTime::now()),
            )
            .await;
        let selector = Selector::new(&registry, Arc::clone(&health), Arc::new(Metrics::new()));
        let probing = Arc::new(Probing::default());
        let route = registry.route("r1").expect("route");
        for _ in 0..4 {
            let pick = selector
                .pick(route, "gpt-5", ApiFormat::ChatCompletions, None, &probing)
                .await;
            assert!(!pick.order().contains(&"p-a".to_string()));
        }
    }
}
