//! 探针的家族适配：Anthropic 版本头、空 api_formats 跳过。

mod common;

use std::time::Duration;

use common::*;
use solidway_core::{Cooldowns, CoreOptions, HealthState, ProviderHealthSeed};

fn options(cooldown: Duration) -> CoreOptions {
    CoreOptions {
        cooldowns: Cooldowns {
            auth: cooldown,
            rate_limit: cooldown,
            unavailable: cooldown,
            quota: cooldown,
        },
        ..fast_options()
    }
}

async fn wait_for<F: FnMut() -> bool>(mut predicate: F) {
    for _ in 0..100 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condition not met");
}

#[tokio::test]
async fn anthropic_probe_uses_messages_and_version_header() {
    let flaky = spawn_mock(Behavior::Status(500, "{}".to_string())).await;
    let good = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            anthropic_provider("flaky", flaky.base_url(), &["claude"]),
            anthropic_provider("good", good.base_url(), &["claude"]),
        ],
        options(Duration::ZERO),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = br#"{"model":"claude","max_tokens":8}"#.to_vec();
    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        body.clone(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(flaky.hit_count(), 1);

    flaky.set_behavior(Behavior::Ok);
    let resp = post_raw(&base, "/v1/messages", &[("x-api-key", CLIENT_KEY)], body).await;
    assert_eq!(resp.status(), 200);

    wait_for(|| {
        flaky
            .requests()
            .iter()
            .any(|request| request.headers.get("anthropic-version").is_some())
    })
    .await;

    let probe = flaky
        .requests()
        .into_iter()
        .find(|request| request.headers.get("anthropic-version").is_some())
        .expect("anthropic probe");
    assert_eq!(probe.uri, "/v1/messages");
    assert_eq!(probe.headers.get("x-api-key").unwrap(), "sk-flaky");
    let body = String::from_utf8_lossy(&probe.body);
    assert!(body.contains("\"max_tokens\":1"), "{body}");
    assert!(body.contains("\"ping\""), "{body}");
}

#[tokio::test]
async fn empty_formats_is_never_probed() {
    let native = spawn_mock(Behavior::Ok).await;
    let good = spawn_mock(Behavior::Ok).await;
    let mut cfg = base_config(
        vec![
            provider_with("native", native.base_url(), &["gpt-5"], vec![], "bearer"),
            provider("good", good.base_url(), &["gpt-5"]),
        ],
        options(Duration::ZERO),
    );
    cfg.health = vec![ProviderHealthSeed {
        provider_id: "native".to_string(),
        state: HealthState::Quarantined {
            until: 0,
            reason: "seed".to_string(),
        },
    }];
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5", "messages": []}),
    )
    .await;
    assert_eq!(resp.status(), 200);
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(native.hit_count(), 0, "空 api_formats 不应发探针");
    assert_eq!(good.hit_count(), 1);
}
