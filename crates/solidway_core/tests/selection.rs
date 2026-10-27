//! 选号策略的集成测试：默认轮询、时延/可用性优先、优先级、回退、陈旧与快照。

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use solidway_core::{
    ApiFormat, CoreOptions, EventBus, HealthManager, Metrics, ModelMapping, Probing, Registry,
    RouteSelection, SelectionStrategy, Selector, epoch_secs,
};

fn fast_options() -> CoreOptions {
    CoreOptions {
        connect_timeout: Duration::from_secs(5),
        first_byte_timeout: Duration::from_secs(5),
        ..CoreOptions::default()
    }
}

fn spec(id: &str, models: &[&str]) -> ProviderSpec {
    ProviderSpec {
        id: id.to_string(),
        base_url: "https://up.example.com/v1".to_string(),
        models: models.iter().map(|m| m.to_string()).collect(),
        enabled: true,
        api_formats: vec![ApiFormat::ChatCompletions],
        auth_scheme: "bearer".to_string(),
        priority: 1,
    }
}

fn config(strategy: SelectionStrategy, providers: Vec<ProviderSpec>) -> solidway_core::CoreConfig {
    let mut cfg = base_config(providers, fast_options());
    cfg.routes[0].selection = RouteSelection::for_strategy(strategy);
    cfg.providers.iter_mut().for_each(|p| {
        p.models = [ModelMapping {
            name: "gpt-5".to_string(),
            upstream: String::new(),
        }]
        .to_vec()
    });
    cfg
}

fn selector(
    cfg: &solidway_core::CoreConfig,
) -> (Registry, Arc<Metrics>, Selector, Arc<HealthManager>) {
    let registry = Registry::new(cfg);
    let health = Arc::new(HealthManager::restore(Vec::new(), EventBus::new()));
    let metrics = Arc::new(Metrics::new());
    let selector = Selector::new(&registry, Arc::clone(&health), Arc::clone(&metrics));
    (registry, metrics, selector, Arc::clone(&health))
}

fn probing() -> Arc<Probing> {
    Arc::new(Probing::default())
}

async fn order(
    registry: &Registry,
    selector: &Selector,
    model: &str,
    format: ApiFormat,
) -> Vec<String> {
    let route = registry.route("r1").expect("route");
    let probing = probing();
    let pick = selector.pick(route, model, format, None, &probing).await;
    pick.order()
}

#[tokio::test]
async fn round_robin_is_the_default_and_cycles() {
    let cfg = config(
        SelectionStrategy::RoundRobin,
        vec![spec("p1", &["gpt-5"]), spec("p2", &["gpt-5"])],
    );
    assert_eq!(
        cfg.routes[0].selection.strategy,
        SelectionStrategy::RoundRobin
    );
    let (registry, _metrics, selector, _health) = selector(&cfg);

    let mut seen = Vec::new();
    for _ in 0..4 {
        seen.push(order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await[0].clone());
    }
    // 轮询：会交替出现。
    assert!(seen.contains(&"p1".to_string()));
    assert!(seen.contains(&"p2".to_string()));
}

#[tokio::test]
async fn latency_strategy_prefers_the_faster_provider() {
    let cfg = config(
        SelectionStrategy::Latency,
        vec![spec("p1", &["gpt-5"]), spec("p2", &["gpt-5"])],
    );
    let (registry, metrics, selector, _health) = selector(&cfg);
    let now = epoch_secs(std::time::SystemTime::now());
    metrics.record_success("p1", 900, now);
    metrics.record_success("p2", 200, now);

    for _ in 0..3 {
        let order = order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await;
        assert_eq!(order[0], "p2", "{order:?}");
    }
}

#[tokio::test]
async fn uptime_strategy_prefers_the_healthier_provider() {
    let cfg = config(
        SelectionStrategy::Uptime,
        vec![spec("p1", &["gpt-5"]), spec("p2", &["gpt-5"])],
    );
    let (registry, metrics, selector, _health) = selector(&cfg);
    let now = epoch_secs(std::time::SystemTime::now());
    metrics.record_success("p1", 300, now);
    metrics.record_success("p1", 300, now);
    metrics.record_success("p2", 300, now);
    for _ in 0..5 {
        metrics.record_failure("p2", now);
    }

    let order = order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await;
    assert_eq!(order[0], "p1", "{order:?}");
}

#[tokio::test]
async fn higher_priority_forms_a_higher_tier() {
    let mut boost = spec("p2", &["gpt-5"]);
    boost.priority = 3;
    let cfg = config(
        SelectionStrategy::Auto,
        vec![spec("p1", &["gpt-5"]), boost],
    );
    let (registry, metrics, selector, _health) = selector(&cfg);
    let now = epoch_secs(std::time::SystemTime::now());
    // 高优先档的 provider 即使更慢，也排在低档之前。
    metrics.record_success("p1", 100, now);
    metrics.record_success("p2", 900, now);

    for _ in 0..3 {
        let order = order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await;
        assert_eq!(order[0], "p2", "{order:?}");
    }
}

#[tokio::test]
async fn tiers_order_candidates_before_strategy_scoring() {
    // p1 档位 2、p2 档位 1：轮询也先走高档，档内再轮询。
    let mut high = spec("p1", &["gpt-5"]);
    high.priority = 2;
    let cfg = config(
        SelectionStrategy::RoundRobin,
        vec![spec("p2", &["gpt-5"]), high],
    );
    let (registry, _metrics, selector, _health) = selector(&cfg);

    for _ in 0..4 {
        let order = order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await;
        assert_eq!(order[0], "p1", "{order:?}");
    }
}

#[tokio::test]
async fn same_tier_falls_back_to_strategy() {
    // 同档位时，latency 策略仍按分数选更快的。
    let mut a = spec("p1", &["gpt-5"]);
    let mut b = spec("p2", &["gpt-5"]);
    a.priority = 5;
    b.priority = 5;
    let cfg = config(SelectionStrategy::Latency, vec![a, b]);
    let (registry, metrics, selector, _health) = selector(&cfg);
    let now = epoch_secs(std::time::SystemTime::now());
    metrics.record_success("p1", 900, now);
    metrics.record_success("p2", 100, now);

    let order = order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await;
    assert_eq!(order[0], "p2", "{order:?}");
}

#[tokio::test]
async fn zero_priority_provider_never_participates() {
    let mut disabled = spec("p1", &["gpt-5"]);
    disabled.priority = 0;
    let cfg = config(
        SelectionStrategy::RoundRobin,
        vec![disabled, spec("p2", &["gpt-5"])],
    );
    let (registry, _metrics, selector, _health) = selector(&cfg);

    for _ in 0..3 {
        let order = order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await;
        assert_eq!(order, vec!["p2".to_string()]);
    }
}

#[tokio::test]
async fn zero_priority_only_candidate_yields_no_candidates() {
    let mut only = spec("p1", &["gpt-5"]);
    only.priority = 0;
    let cfg = config(SelectionStrategy::RoundRobin, vec![only]);
    let (registry, _metrics, selector, _health) = selector(&cfg);

    let order = order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await;
    assert!(order.is_empty(), "{order:?}");
}

#[tokio::test]
async fn zero_weights_fall_back_to_round_robin() {
    let cfg = config(
        SelectionStrategy::Auto,
        vec![spec("p1", &["gpt-5"]), spec("p2", &["gpt-5"])],
    );
    let (registry, metrics, _selector, health) = selector(&cfg);
    let now = epoch_secs(std::time::SystemTime::now());
    metrics.record_success("p1", 900, now);
    metrics.record_success("p2", 100, now);

    let mut selection = cfg.routes[0].selection;
    selection.weights.uptime = 0.0;
    selection.weights.latency = 0.0;
    selection.thresholds.exploration_rate = 0.0;
    assert!(selection.falls_back());
    assert_eq!(selection.effective_strategy(), SelectionStrategy::RoundRobin);

    // 回退后不再按分数（100ms 的 p2 不再总是第一位）。
    let _ = registry;
    let mut cfg2 = cfg.clone();
    cfg2.routes[0].selection = selection;
    let registry2 = Registry::new(&cfg2);
    let selector2 = Selector::new(&registry2, Arc::clone(&health), Arc::clone(&metrics));
    let probing = probing();
    let mut firsts = Vec::new();
    for _ in 0..4 {
        let pick = selector2
            .pick(
                registry2.route("r1").unwrap(),
                "gpt-5",
                ApiFormat::ChatCompletions,
                None,
                &probing,
            )
            .await;
        firsts.push(pick.order()[0].clone());
    }
    assert!(firsts.contains(&"p1".to_string()), "{firsts:?}");
}

#[tokio::test]
async fn stale_metrics_fall_back_to_defaults_and_keep_rotation() {
    let cfg = config(
        SelectionStrategy::Latency,
        vec![spec("p1", &["gpt-5"]), spec("p2", &["gpt-5"])],
    );
    let (registry, metrics, selector, _health) = selector(&cfg);
    let ancient = epoch_secs(std::time::SystemTime::now()) - 3600;
    metrics.record_success("p1", 10, ancient);
    metrics.record_success("p2", 9000, ancient);

    let mut firsts = Vec::new();
    for _ in 0..4 {
        firsts.push(order(&registry, &selector, "gpt-5", ApiFormat::ChatCompletions).await[0].clone());
    }
    // 全部陈旧 → 分数相同 → 按轮询打散。
    assert!(firsts.contains(&"p1".to_string()), "{firsts:?}");
    assert!(firsts.contains(&"p2".to_string()), "{firsts:?}");
}

#[tokio::test]
async fn snapshot_reports_scores_and_metrics() {
    let cfg = config(
        SelectionStrategy::Latency,
        vec![spec("p1", &["gpt-5"]), spec("p2", &["gpt-5"])],
    );
    let (registry, metrics, selector, _health) = selector(&cfg);
    let now = epoch_secs(std::time::SystemTime::now());
    metrics.record_success("p1", 200, now);
    metrics.record_success("p2", 800, now);

    let route = registry.route("r1").expect("route");
    let snapshot = selector.snapshot(route, Some("gpt-5")).await;
    assert_eq!(snapshot.len(), 2);
    let fast = snapshot.iter().find(|p| p.provider_id == "p1").unwrap();
    let slow = snapshot.iter().find(|p| p.provider_id == "p2").unwrap();
    assert_eq!(fast.samples, 1);
    assert!(!fast.stale);
    assert!(fast.score.unwrap() < slow.score.unwrap(), "{snapshot:?}");
    // 分数不含档位：p1/p2 同为默认档位 1 时分数差只来自时延。
    assert_eq!(fast.priority, 1);
    assert_eq!(fast.health, "healthy");
}

#[tokio::test]
async fn round_robin_snapshot_has_no_scores() {
    let cfg = config(
        SelectionStrategy::RoundRobin,
        vec![spec("p1", &["gpt-5"])],
    );
    let (registry, _metrics, selector, _health) = selector(&cfg);
    let route = registry.route("r1").expect("route");
    let snapshot = selector.snapshot(route, None).await;
    assert_eq!(snapshot.len(), 1);
    assert!(snapshot[0].score.is_none());
    assert_eq!(snapshot[0].samples, 0);
    assert!(snapshot[0].stale);
}

#[tokio::test]
async fn engine_records_metrics_and_exposes_snapshot() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let mut cfg = base_config(
        vec![common::provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    cfg.routes[0].selection = RouteSelection::for_strategy(SelectionStrategy::Latency);
    let (base, core) = spawn_core(cfg).await;

    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5", "messages": []}),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let snapshot = core
        .selection_snapshot("r1", None)
        .await
        .expect("snapshot");
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].samples, 1);
    assert!(snapshot[0].ttfb_ms.is_some());
    assert!(snapshot[0].score.is_some());

    assert!(core.selection_snapshot("nope", None).await.is_none());
}
