//! 会话粘性集成测试（tasks 4.1，session-affinity spec）。

mod common;

use std::time::Duration;

use common::{ClientRequest, Harness, MockResponse};
use serde_json::json;

const TENANT_KEY: &str = "gw-tenant";

/// `/v1/responses` 请求：`previous_response_id` 即会话标识。
fn responses_request(session: &str) -> ClientRequest {
    ClientRequest::post_json(
        "/v1/responses",
        &json!({
            "model": "gpt-5",
            "previous_response_id": session,
            "input": "hello"
        }),
    )
    .bearer(TENANT_KEY)
}

#[tokio::test]
async fn same_session_reuses_same_provider() {
    let harness = Harness::two_providers("affinity-hit", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    // 首个请求落在一个 provider 上。
    let first = harness.client.send(responses_request("sess-1")).await;
    assert_eq!(first.status, 200);
    let (first_provider, other) = if harness.mocks.a.count() == 1 {
        (&harness.mocks.a, &harness.mocks.b)
    } else {
        (&harness.mocks.b, &harness.mocks.a)
    };

    // 同会话后续请求必须复用同一 provider（轮询计数器不得抢走它）。
    for _ in 0..3 {
        let response = harness.client.send(responses_request("sess-1")).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(first_provider.count(), 4, "同会话应复用同一 provider");
    assert_eq!(other.count(), 0, "其他 provider 不应承载该会话");
}

#[tokio::test]
async fn different_sessions_can_use_different_providers() {
    let harness = Harness::two_providers("affinity-split", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    // 无粘性的两个会话各自轮询选号，应分散到两个 provider。
    let first = harness.client.send(responses_request("sess-a")).await;
    assert_eq!(first.status, 200);
    let second = harness.client.send(responses_request("sess-b")).await;
    assert_eq!(second.status, 200);

    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1);
}

#[tokio::test]
async fn affinity_is_isolated_per_model_within_the_same_session() {
    // 路由对外提供模型并集：同一会话跨模型请求时，粘性映射 MUST 按 (路由, 模型) 隔离。
    // 刻意让两个模型的候选集不同（gpt-5 → [p-a, p-b]，gpt-5-mini → [p-b]）。
    let harness = common::build_harness("affinity-per-model", "", "/v1", |m| {
        vec![
            common::SeedRoute::new("r-main", "main")
                .provider("p-a", m.a.base_url(), &["gpt-5"])
                .provider("p-b", m.b.base_url(), &["gpt-5", "gpt-5-mini"])
                .key("k-main", TENANT_KEY),
        ]
    })
    .await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let request = |model: &str| {
        ClientRequest::post_json(
            "/v1/responses",
            &json!({
                "model": model,
                "previous_response_id": "sess-cross",
                "input": "hello"
            }),
        )
        .bearer(TENANT_KEY)
    };

    assert_eq!(harness.client.send(request("gpt-5")).await.status, 200);
    assert_eq!(harness.client.send(request("gpt-5-mini")).await.status, 200);
    assert_eq!(harness.client.send(request("gpt-5")).await.status, 200);
    assert_eq!(harness.client.send(request("gpt-5-mini")).await.status, 200);

    assert_eq!(harness.mocks.a.count(), 2, "gpt-5 的两条请求都应复用 p-a");
    assert_eq!(harness.mocks.b.count(), 2, "gpt-5-mini 的两条请求都应复用 p-b");
}

#[tokio::test]
async fn affinity_follows_provider_switch_after_quarantine() {
    let harness = Harness::two_providers("affinity-follow", "").await;
    // provider A 首次成功（建立粘性）→ 之后 429（被移出）。
    harness.mocks.a.script(vec![MockResponse::ok("gpt-5")]);
    harness
        .mocks
        .a
        .set_fallback(MockResponse::rate_limited(None));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(responses_request("sess-2")).await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.mocks.a.count(), 1, "首个请求应由 A 承载");
    assert_eq!(harness.mocks.b.count(), 0);

    // A 被 429 移出，本次请求换由 B 承载成功。
    let switched = harness.client.send(responses_request("sess-2")).await;
    assert_eq!(switched.status, 200);
    assert_eq!(harness.mocks.b.count(), 1, "换号应落到 B");

    // 映射已跟随 B：后续同会话请求继续由 B 承载，不再回到 A。
    let after = harness.client.send(responses_request("sess-2")).await;
    assert_eq!(after.status, 200);
    assert_eq!(
        harness.mocks.b.count(),
        2,
        "换号后粘性映射必须跟随新 provider"
    );
    assert_eq!(
        harness.mocks.a.count(),
        2,
        "A 只应被尝试过两次（首次 + 换号前那次）"
    );
}

#[tokio::test]
async fn affinity_is_not_updated_when_switch_fails() {
    let harness = Harness::two_providers("affinity-stale", "").await;
    // A 先成功建立粘性，随后 A、B 都因配额耗尽进入长冷却（候选集合耗尽）。
    harness.mocks.a.script(vec![MockResponse::ok("gpt-5")]);
    harness
        .mocks
        .a
        .set_fallback(MockResponse::insufficient_quota());
    harness
        .mocks
        .b
        .set_fallback(MockResponse::insufficient_quota());

    let first = harness.client.send(responses_request("sess-3")).await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.mocks.a.count(), 1, "首个请求由 A 承载");

    // 本次请求：A 退役 → 换 B → B 也退役 → 全失败（换号未成功）。
    let failed = harness.client.send(responses_request("sess-3")).await;
    assert_eq!(failed.status, 429, "全失败应回传最后一个上游响应");
    assert_eq!(harness.mocks.a.count(), 2, "A 被尝试过一次（退役判定）");
    assert_eq!(harness.mocks.b.count(), 1, "B 被尝试过一次（退役判定）");
    let rows = common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| rows.len() == 2).await;
    assert_eq!(rows.len(), 2, "两个 provider 都应已落库为不可用：{rows:?}");
    assert!(
        rows.iter().all(|(_, status, _)| status == "quarantined"),
        "配额耗尽应为冷却而非退役：{rows:?}"
    );

    // 换号未成功 → 粘性映射保持指向原 provider；候选集合内已无健康 provider。
    let exhausted = harness.client.send(responses_request("sess-3")).await;
    assert_eq!(exhausted.status, 503, "候选集合内已无健康 provider");
    assert_eq!(
        exhausted
            .json()
            .pointer("/error/code")
            .and_then(|v| v.as_str()),
        Some("route_exhausted")
    );
    assert_eq!(harness.mocks.a.count(), 2, "候选集合耗尽后不得再打 A");
    assert_eq!(harness.mocks.b.count(), 1, "B 只被尝试过一次");
}

#[tokio::test]
async fn affinity_hits_renew_the_sliding_ttl() {
    // 滑动 TTL：每次命中续期，因此在远超单个 TTL 的时间窗内仍复用同一 provider。
    // 间隔取 1s、TTL 取 5s，留足请求开销的余量（避免把「本来就会过期」测成续期）。
    let harness = Harness::two_providers("affinity-renew", "affinity_ttl = \"5s\"").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(responses_request("sess-5")).await;
    assert_eq!(first.status, 200);
    let (sticky, other) = if harness.mocks.a.count() == 1 {
        (&harness.mocks.a, &harness.mocks.b)
    } else {
        (&harness.mocks.b, &harness.mocks.a)
    };

    // 每次间隔 1s（< TTL），命中续期后仍应复用。
    for _ in 0..3 {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let response = harness.client.send(responses_request("sess-5")).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(sticky.count(), 4, "TTL 内每次命中都应续期复用");
    assert_eq!(other.count(), 0, "续期期间不得换号");
}

#[tokio::test]
async fn expired_affinity_returns_to_round_robin() {
    // 超过 TTL 未被命中 → 条目清除、重新按选号策略选号；
    // 与长 TTL 的对照组相比，请求会重新分散到多个 provider。
    let short = Harness::two_providers("affinity-expire-short", "affinity_ttl = \"1s\"").await;
    for mock in [&short.mocks.a, &short.mocks.b] {
        mock.set_fallback(MockResponse::ok("gpt-5"));
    }
    short.client.send(responses_request("sess-6")).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    for _ in 0..3 {
        let response = short.client.send(responses_request("sess-6")).await;
        assert_eq!(response.status, 200);
    }
    let short_total = short.mocks.a.count() + short.mocks.b.count();
    assert_eq!(short_total, 4);
    assert!(
        short.mocks.a.count() > 0 && short.mocks.b.count() > 0,
        "过期后应重新按轮询选号（不再固定同一 provider）：a={} b={}",
        short.mocks.a.count(),
        short.mocks.b.count()
    );

    let long = Harness::two_providers("affinity-expire-long", "affinity_ttl = \"1h\"").await;
    for mock in [&long.mocks.a, &long.mocks.b] {
        mock.set_fallback(MockResponse::ok("gpt-5"));
    }
    long.client.send(responses_request("sess-6")).await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    for _ in 0..3 {
        let response = long.client.send(responses_request("sess-6")).await;
        assert_eq!(response.status, 200);
    }
    let (sticky, other) = if long.mocks.a.count() == 4 {
        (&long.mocks.a, &long.mocks.b)
    } else {
        (&long.mocks.b, &long.mocks.a)
    };
    assert_eq!(sticky.count(), 4, "长 TTL 下应始终复用同一 provider");
    assert_eq!(other.count(), 0);
}

#[tokio::test]
async fn chat_completions_uses_prompt_cache_key_for_affinity() {
    let harness = Harness::two_providers("affinity-chat", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let request = || {
        ClientRequest::post_json(
            "/v1/chat/completions",
            &json!({
                "model": "gpt-5",
                "prompt_cache_key": "cache-sess",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        )
        .bearer(TENANT_KEY)
    };

    let first = harness.client.send(request()).await;
    assert_eq!(first.status, 200);
    let (sticky, other) = if harness.mocks.a.count() == 1 {
        (&harness.mocks.a, &harness.mocks.b)
    } else {
        (&harness.mocks.b, &harness.mocks.a)
    };

    for _ in 0..2 {
        let response = harness.client.send(request()).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(
        sticky.count(),
        3,
        "chat completions 的 prompt_cache_key 也应建立粘性"
    );
    assert_eq!(other.count(), 0);
}

#[tokio::test]
async fn requests_without_session_identifier_are_not_sticky() {
    let harness = Harness::two_providers("affinity-none", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let request = || {
        ClientRequest::post_json(
            "/v1/chat/completions",
            &json!({
                "model": "gpt-5",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        )
        .bearer(TENANT_KEY)
    };

    for _ in 0..4 {
        let response = harness.client.send(request()).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(harness.mocks.a.count(), 2, "无会话标识时按轮询均衡分布");
    assert_eq!(harness.mocks.b.count(), 2);
}
