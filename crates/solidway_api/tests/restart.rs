//! 重启连续性：收尾落盘用量、写运行期快照，启动时恢复指标与粘性种子。

mod common;

use std::time::Duration;

use common::{ClientRequest, Harness, MockResponse, SeedRoute};
use serde_json::json;

const GATEWAY_KEY: &str = "gw-tenant";

async fn harness(tag: &str) -> Harness {
    let harness = Harness::two_providers(tag, "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));
    harness
}

/// 带会话标识的 chat 请求（`prompt_cache_key` 进粘性）。
async fn chat_with_session(harness: &Harness, session: &str) {
    let body = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "hello"}],
        "prompt_cache_key": session,
    });
    let response = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &body).bearer(GATEWAY_KEY))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
}

#[tokio::test]
async fn app_restart_signals_unified_shutdown_channel() {
    let harness = common::build_harness_with_mode(
        "restart-channel",
        "",
        "/v1",
        solidway_api::RunMode::Desktop,
        |mocks| {
            vec![
                SeedRoute::new("r-main", "main")
                    .provider("p-a", mocks.a.base_url(), &["gpt-5"])
                    .key("k-main", GATEWAY_KEY),
            ]
        },
    )
    .await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));

    let (tx, mut rx) = tokio::sync::watch::channel(false);
    harness.api.state().install_shutdown(tx);
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(ClientRequest::new("POST", "/api/app/restart").header("cookie", &cookie))
        .await;
    assert_eq!(response.status, 202, "{}", response.text());

    // 与 SIGTERM 同语义：handler 只触发统一通道，由 main 负责停接、等在途与收尾。
    tokio::time::timeout(Duration::from_secs(2), rx.wait_for(|value| *value))
        .await
        .expect("restart 应触发统一关闭通道")
        .expect("关闭通道应保持可读");
}

#[tokio::test]
async fn shutdown_writes_usage_and_snapshot() {
    let harness = harness("restart-shutdown").await;
    chat_with_session(&harness, "s-1").await;
    // 等一次事件泵消费（事件是异步的）。
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    harness.api.state().graceful_shutdown().await;

    // 用量已落盘：当前小时桶有 1 次尝试。
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT COALESCE(SUM(attempts), 0) FROM usage_attempt_bucket WHERE granularity = 'hour'",
    )
    .fetch_all(&harness.api.state().pool)
    .await
    .expect("query attempt bucket");
    assert!(rows[0].0 >= 1, "收尾后应已落盘尝试计数");

    // 快照已写入，且包含该会话的粘性条目。
    let payload = solidway_api::db::runtime_snapshot::load(&harness.api.state().pool).await;
    assert_eq!(payload.affinity.len(), 1, "{:?}", payload.affinity);
    assert_eq!(payload.affinity[0].session, "s-1");
}

#[tokio::test]
async fn startup_config_carries_metric_and_affinity_seeds() {
    let harness = harness("restart-seeds").await;
    chat_with_session(&harness, "s-2").await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    harness.api.state().graceful_shutdown().await;

    let startup = solidway_api::core_bridge::build_startup_config(harness.api.state())
        .await
        .expect("startup config");
    assert_eq!(startup.affinity.len(), 1);
    assert_eq!(startup.affinity[0].session, "s-2");
    assert!(!startup.metrics.is_empty(), "应从尝试桶聚合出指标种子");
    let seed = &startup.metrics[0];
    assert!(seed.samples >= 1);
    assert!(seed.ttfb_ms >= 0.0);
    assert!((0.0..=1.0).contains(&seed.failure_rate));

    // 运行期下发（apply_config 用）不带任何种子。
    let runtime = solidway_api::core_bridge::build_core_config(harness.api.state())
        .await
        .expect("runtime config");
    assert!(runtime.metrics.is_empty());
    assert!(runtime.affinity.is_empty());
}

#[tokio::test]
async fn restored_affinity_keeps_the_same_provider() {
    let harness = harness("restart-affinity").await;
    // 两次请求同一会话：都应由同一 provider 承载（粘性生效）。
    chat_with_session(&harness, "s-3").await;
    chat_with_session(&harness, "s-3").await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let exported = harness.gateway.export_affinity(100).await;
    assert_eq!(exported.len(), 1);
    let provider_id = exported[0].provider_id.clone();

    // 收尾写快照（重启前的正常流程），再用启动配置构造新 core（模拟重启）。
    harness.api.state().graceful_shutdown().await;
    let startup = solidway_api::core_bridge::build_startup_config(harness.api.state())
        .await
        .expect("startup config");
    let core = solidway_core::Core::new(startup).await.expect("new core");
    let snapshot = core
        .selection_snapshot(exported[0].route_id.as_str(), Some("gpt-5"))
        .await
        .expect("snapshot");
    assert!(snapshot.iter().any(|p| p.provider_id == provider_id));
    // 种子已进入新实例的粘性表（导出应与原一致）。
    let restored = core.export_affinity(100).await;
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].provider_id, provider_id);
}
