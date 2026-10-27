//! provider 健康与探测回池、会话粘性的集成测试（移植 develop `tests/probe/affinity` 语义）。

mod common;

use common::*;
use solidway_core::{Core, CoreEvent, CoreOptions};
use std::time::Duration;

fn options(cooldown: Duration) -> CoreOptions {
    CoreOptions {
        connect_timeout: Duration::from_secs(5),
        first_byte_timeout: Duration::from_secs(5),
        cooldowns: solidway_core::Cooldowns {
            auth: cooldown,
            rate_limit: cooldown,
            unavailable: cooldown,
            quota: cooldown,
        },
        ..CoreOptions::default()
    }
}

/// 等某个 provider 的第 `want` 次目标健康迁移事件；超时 panic 并打印观测值。
async fn wait_for_health_events(
    events: &mut tokio::sync::broadcast::Receiver<CoreEvent>,
    provider: &str,
    state: &str,
    want: usize,
    mock: &Mock,
    core: &Core,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut seen = 0usize;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(CoreEvent::HealthChanged(event)))
                if event.provider_id == provider && event.state.as_str() == state =>
            {
                seen += 1;
                if seen >= want {
                    return;
                }
            }
            Ok(Ok(_)) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                panic!("health event channel closed while waiting for {provider} {state}");
            }
            Err(_) => panic!(
                "timed out waiting for {provider} {state} x{want} (seen={seen}, hit_count={}, probe_count={})",
                mock.hit_count(),
                core.probe_count()
            ),
        }
    }
}

/// flaky 收到的**转发**请求数（探针 body 含 `"ping"`，据此区分探针与转发）。
fn forwarded_request_count(mock: &Mock) -> usize {
    mock.requests()
        .iter()
        .filter(|request| !String::from_utf8_lossy(&request.body).contains("ping"))
        .count()
}

#[tokio::test]
async fn marked_provider_is_excluded_and_failover_continues() {
    let bad = spawn_mock(Behavior::Status(401, "{}".to_string())).await;
    let good = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            provider("bad", bad.base_url(), &["gpt-5"]),
            provider("good", good.base_url(), &["gpt-5"]),
        ],
        options(Duration::from_secs(1800)),
    );
    let (base, _core) = spawn_core(cfg).await;

    // 第一次：先打 bad（401 → 冷却）→ 换号到 good。
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);

    // 第二次：bad 已冷却 → 直接用 good。
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);

    assert_eq!(bad.hit_count(), 1, "冷却中的凭据不再被选中");
    assert_eq!(good.hit_count(), 2);
}

#[tokio::test]
async fn probe_recovers_provider_after_cooldown() {
    // 冷却时长设为 0：标记后立即到期，下一次请求会触发探针。
    let flaky = spawn_mock(Behavior::Status(500, "{}".to_string())).await;
    let good = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            provider("flaky", flaky.base_url(), &["gpt-5"]),
            provider("good", good.base_url(), &["gpt-5"]),
        ],
        options(Duration::from_secs(0)),
    );
    let (base, core) = spawn_core(cfg).await;

    // 第一次：flaky 500 → 冷却（已到期）→ 换号到 good。
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(flaky.hit_count(), 1);

    // 触发探针前订阅：探针通过回池会发 flaky 的 Healthy 迁移。
    let mut events = core.subscribe();

    // flaky 恢复健康；下一次请求触发探针。
    flaky.set_behavior(Behavior::Ok);
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);
    wait_for_health_events(&mut events, "flaky", "healthy", 1, &flaky, &core).await;

    // 探针已通过 → flaky 回池 → 后续请求可能再次命中 flaky。
    let before = flaky.hit_count();
    for _ in 0..4 {
        let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
        assert_eq!(resp.status(), 200);
    }
    assert!(
        flaky.hit_count() > before,
        "探针通过后 flaky 应重新参与选号（before={before}, after={}）",
        flaky.hit_count()
    );
}

#[tokio::test]
async fn probe_body_timeout_does_not_recover_provider() {
    let flaky = spawn_mock(Behavior::Status(500, "{}".to_string())).await;
    let good = spawn_mock(Behavior::Ok).await;
    let mut opts = options(Duration::from_secs(1));
    opts.response_body_timeout = Duration::from_millis(150);
    let cfg = base_config(
        vec![
            provider("flaky", flaky.base_url(), &["gpt-5"]),
            provider("good", good.base_url(), &["gpt-5"]),
        ],
        opts,
    );
    let (base, core) = spawn_core(cfg).await;
    // 任何请求之前订阅：首次 500 冷却与探针重冷却两次 Quarantined 都会入队。
    let mut events = core.subscribe();

    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(flaky.hit_count(), 1);

    // 探针返回 200 头，但响应体挂住：读取超时不得视为探针成功。
    flaky.set_behavior(Behavior::SlowBody {
        status: 200,
        body: "{}".to_string(),
        delay: Duration::from_secs(3),
    });

    // 以请求循环等冷却到期并触发探针，直到探针命中 flaky（hit_count 1 → 2）。
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while flaky.hit_count() < 2 {
        assert!(
            tokio::time::Instant::now() < deadline,
            "探针未启动（hit_count={}, probe_count={}）",
            flaky.hit_count(),
            core.probe_count()
        );
        let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
        assert_eq!(resp.status(), 200);
        if flaky.hit_count() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(flaky.hit_count() >= 2, "探针未启动");

    // 等探针响应体读取超时并重新进入冷却：事件流里先后有首次 500 冷却与探针重冷却
    // 两次 Quarantined，等满两次才算分类完成（容量 4096，积压消费安全）。
    wait_for_health_events(&mut events, "flaky", "quarantined", 2, &flaky, &core).await;

    let snapshot = core
        .selection_snapshot("r1", Some("gpt-5"))
        .await
        .expect("snapshot");
    let flaky_health = snapshot
        .into_iter()
        .find(|provider| provider.provider_id == "flaky")
        .map(|provider| provider.health)
        .unwrap_or_default();
    assert_eq!(flaky_health, "quarantined", "响应体超时后必须重新冷却");
    let before = flaky.hit_count();
    let forwarded_before = forwarded_request_count(&flaky);
    for _ in 0..2 {
        let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
        assert_eq!(resp.status(), 200);
    }
    assert_eq!(
        forwarded_request_count(&flaky),
        forwarded_before,
        "探针响应体读取超时不得回池：最后 2 个请求不得转发命中 flaky"
    );
    assert_eq!(
        flaky.hit_count(),
        before,
        "探针响应体读取超时必须判失败，不能把 Provider 放回池"
    );
}

#[tokio::test]
async fn affinity_reuses_same_provider_for_session() {
    let a = spawn_mock(Behavior::Ok).await;
    let b = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            provider("a", a.base_url(), &["gpt-5"]),
            provider("b", b.base_url(), &["gpt-5"]),
        ],
        options(Duration::from_secs(1800)),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = serde_json::json!({"model": "gpt-5", "prompt_cache_key": "sess-1"});
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body.clone()).await;
    assert_eq!(resp.status(), 200);
    let first = if a.hit_count() > 0 { "a" } else { "b" };

    for _ in 0..3 {
        let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body.clone()).await;
        assert_eq!(resp.status(), 200);
    }

    if first == "a" {
        assert_eq!(a.hit_count(), 4, "同会话复用同一凭据");
        assert_eq!(b.hit_count(), 0);
    } else {
        assert_eq!(b.hit_count(), 4);
        assert_eq!(a.hit_count(), 0);
    }
}
