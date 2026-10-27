//! 探测回池集成测试（tasks 5.1 / 5.2，account-health「探测回池」+ design.md D5）。

mod common;

use std::time::Duration;

use common::{ClientRequest, Harness, MockResponse};
use serde_json::json;

const TENANT_KEY: &str = "gw-tenant";

fn chat(model: &str) -> ClientRequest {
    ClientRequest::post_json(
        "/v1/chat/completions",
        &json!({
            "model": model,
            "messages": [{"role": "user", "content": "hello"}]
        }),
    )
    .bearer(TENANT_KEY)
}

/// 冷却取 1s：403 之后等 1.2s 即可让 provider 进入「冷却到期、待探针」状态。
/// （401 现在是退役，不参与探针流程。）
const SHORT_AUTH_COOLDOWN: &str = "cooldown_auth = \"1s\"";

#[tokio::test]
async fn probe_pass_returns_provider_to_the_pool() {
    let harness = Harness::two_providers("probe-pass", SHORT_AUTH_COOLDOWN).await;
    // A 首次 401（被移出）→ 之后一直成功（探针通过 + 回池后承载）。
    harness.mocks.a.script(vec![MockResponse::forbidden()]);
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    // 首个请求：A 失败被移出，换 B 成功。
    let first = harness.client.send(chat("gpt-5")).await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1);
    // 健康落库经事件异步完成：轮询等待而非立刻读库（避免竞态）。
    let rows = common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| rows.len() == 1).await;
    assert_eq!(rows.len(), 1, "A 应已落库为临时移出：{rows:?}");
    assert_eq!(rows[0].0, "p-a");
    assert_eq!(rows[0].1, "quarantined");

    // 等冷却到期。
    tokio::time::sleep(Duration::from_millis(1200)).await;

    // 冷却到期的 provider 不直接回池：先探针。B 备足脚本（请求 + 探针可能的并发）。
    harness.mocks.b.script(vec![MockResponse::ok("gpt-5"); 3]);
    let second = harness.client.send(chat("gpt-5")).await;
    assert_eq!(second.status, 200);
    assert!(
        common::wait_for(Duration::from_secs(3), || harness.gateway.probe_count()
            >= 1)
        .await,
        "冷却到期后应发起探针"
    );
    // 等探针**到达** A（spawn 与到达之间有窗口，不能只看发起计数）。
    assert!(
        common::wait_for(Duration::from_secs(3), || harness.mocks.a.count() >= 2).await,
        "第二次到达 A 的请求应是探针"
    );
    // 等探针**完成**（落库行被清除），而不是仅发起。
    let cleared = common::wait_for_rows(
        &harness.db_path,
        Duration::from_secs(3),
        |rows: &[(String, String, Option<i64>)]| rows.is_empty(),
    )
    .await;
    assert!(cleared.is_empty(), "探针通过后应清空健康标记：{cleared:?}");

    // 探针请求的形状：极简测试请求（max_tokens: 1，使用模型清单首个模型名）。
    let probe = harness.mocks.a.last_request().expect("探针请求");
    assert_eq!(probe.method, "POST");
    assert_eq!(probe.path, "/v1/chat/completions");
    assert_eq!(probe.header("authorization"), Some("Bearer sk-provider-a"));
    let probe_body = probe.json();
    assert_eq!(
        probe_body.get("model").and_then(|v| v.as_str()),
        Some("gpt-5"),
        "探针必须使用该 provider 模型清单的首个模型名"
    );
    assert_eq!(
        probe_body.get("max_tokens").and_then(|v| v.as_u64()),
        Some(1),
        "探针必须是极小生成"
    );

    // 探针通过 → 清标记回池 → 后续请求由 A 承载（粘性优先，同一会话）。
    harness.mocks.a.script(vec![MockResponse::ok("gpt-5")]);
    let third = harness.client.send(chat("gpt-5")).await;
    assert_eq!(third.status, 200);
    assert_eq!(
        harness.mocks.a.count(),
        3,
        "回池后 A 应重新参与承载（a={} b={}）",
        harness.mocks.a.count(),
        harness.mocks.b.count()
    );
}

#[tokio::test]
async fn probe_failure_requarantines_provider() {
    let harness = Harness::two_providers("probe-503", SHORT_AUTH_COOLDOWN).await;
    harness.mocks.a.script(vec![MockResponse::forbidden()]);
    harness.mocks.a.set_fallback(MockResponse::overloaded());
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(chat("gpt-5")).await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.mocks.a.count(), 1);

    tokio::time::sleep(Duration::from_millis(1200)).await;
    harness.mocks.b.script(vec![MockResponse::ok("gpt-5"); 3]);
    let second = harness.client.send(chat("gpt-5")).await;
    assert_eq!(second.status, 200);
    assert!(
        common::wait_for(Duration::from_secs(3), || harness.mocks.a.count() >= 2).await,
        "应发起探针"
    );

    // 探针 503 → 按分类表重新临时移出（冷却 1m，不会立刻再探）。
    let stored = common::wait_for_rows(&harness.db_path, Duration::from_secs(3), |rows| {
        rows.iter()
            .any(|(provider, status, _)| provider == "p-a" && status == "quarantined")
    })
    .await;
    assert_eq!(stored.len(), 1, "探针失败后必须重新落库：{stored:?}");
    assert_eq!(stored[0].1, "quarantined");

    // provider 仍不参与选号：后续请求不应打到 A（除探针外）。
    let before = harness.mocks.a.count();
    for _ in 0..2 {
        let response = harness.client.send(chat("gpt-5")).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(
        harness.mocks.a.count(),
        before,
        "重新冷却的 provider 不得承载正式请求"
    );
}

#[tokio::test]
async fn probe_insufficient_quota_keeps_provider_in_long_cooldown() {
    let harness = Harness::two_providers("probe-quota", SHORT_AUTH_COOLDOWN).await;
    harness.mocks.a.script(vec![MockResponse::forbidden()]);
    harness
        .mocks
        .a
        .set_fallback(MockResponse::insufficient_quota());
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(chat("gpt-5")).await;
    assert_eq!(first.status, 200);

    tokio::time::sleep(Duration::from_millis(1200)).await;
    harness.mocks.b.script(vec![MockResponse::ok("gpt-5"); 3]);
    let second = harness.client.send(chat("gpt-5")).await;
    assert_eq!(second.status, 200);
    assert!(
        common::wait_for(Duration::from_secs(3), || harness.mocks.a.count() >= 2).await,
        "应发起探针"
    );

    // 配额耗尽不再永久退役：进入长冷却（默认 6h），因此仍是 quarantined。
    let stored = common::wait_for_rows(&harness.db_path, Duration::from_secs(3), |rows| {
        rows.iter()
            .any(|(provider, status, _)| provider == "p-a" && status == "quarantined")
    })
    .await;
    assert!(
        stored
            .iter()
            .any(|(provider, status, _)| provider == "p-a" && status == "quarantined"),
        "探针打到 insufficient_quota 应进入长冷却而非退役：{stored:?}"
    );

    // 长冷却期间不再被选号。
    let before = harness.mocks.a.count();
    for _ in 0..2 {
        let response = harness.client.send(chat("gpt-5")).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(harness.mocks.a.count(), before);
}

#[tokio::test]
async fn probe_with_unmarked_failure_keeps_provider_unavailable() {
    let harness = Harness::two_providers("probe-400", SHORT_AUTH_COOLDOWN).await;
    // A：先 401 被移出，之后每次都按 400（不标记类）响应——探针永远不通过。
    harness.mocks.a.script(vec![MockResponse::forbidden()]);
    harness.mocks.a.set_fallback(MockResponse::bad_request());
    // B：一直成功；用可识别响应头区分「B 的正式响应」。
    harness
        .mocks
        .b
        .set_fallback(MockResponse::ok("gpt-5").with_header("x-served-by", "p-b"));

    let first = harness.client.send(chat("gpt-5")).await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1);

    // 等冷却到期，再请求一次以触发惰性探针。
    tokio::time::sleep(Duration::from_millis(1200)).await;
    let second = harness.client.send(chat("gpt-5")).await;
    assert_eq!(second.status, 200);
    assert_eq!(
        second.header("x-served-by"),
        Some("p-b"),
        "探针未通过的 provider 即使冷却到期也不得承接正式请求"
    );
    assert!(
        common::wait_for(Duration::from_secs(3), || harness.gateway.probe_count()
            >= 1)
        .await,
        "应发起探针"
    );
    // 探针未通过且无标记：provider 保持不可用（仍为临时移出）。
    let rows = common::wait_for_rows(&harness.db_path, Duration::from_secs(3), |rows| {
        rows.iter()
            .any(|(provider, status, _)| provider == "p-a" && status == "quarantined")
    })
    .await;
    assert!(
        rows.iter()
            .any(|(provider, status, _)| provider == "p-a" && status == "quarantined"),
        "探针未通过且不标记时 provider 应保持不可用：{rows:?}"
    );

    // 后续请求仍全部由 B 承载，且 A 的每次命中都必须是探针（max_tokens: 1）。
    let a_before = harness.mocks.a.count();
    let b_before = harness.mocks.b.count();
    let mut observed_probes = 0usize;
    for _ in 0..2 {
        let response = harness.client.send(chat("gpt-5")).await;
        assert_eq!(response.status, 200, "客户端不得看到上游 400");
        assert_eq!(response.header("x-served-by"), Some("p-b"));
    }
    assert_eq!(
        harness.mocks.b.count(),
        b_before + 2,
        "正式请求必须全部由健康 provider 承载"
    );
    for request in harness.mocks.a.requests().iter().skip(a_before) {
        assert_eq!(
            request.json().get("max_tokens").and_then(|v| v.as_u64()),
            Some(1),
            "未通过探针的 provider 只能被探针命中"
        );
        observed_probes += 1;
    }
    assert!(observed_probes >= 1, "后续选号应惰性重探");
}

#[tokio::test]
async fn expired_cooldown_does_not_return_provider_without_probe() {
    let harness = Harness::two_providers("probe-gate", SHORT_AUTH_COOLDOWN).await;
    harness.mocks.a.script(vec![MockResponse::forbidden()]);
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(chat("gpt-5")).await;
    assert_eq!(first.status, 200);
    let a_after_first = harness.mocks.a.count();
    assert_eq!(a_after_first, 1);

    tokio::time::sleep(Duration::from_millis(1200)).await;

    // 冷却到期后，A 不直接承接正式请求：它的下一次命中必须是探针（形状可判定）。
    harness.mocks.b.script(vec![MockResponse::ok("gpt-5"); 3]);
    let second = harness.client.send(chat("gpt-5")).await;
    assert_eq!(second.status, 200);
    assert!(
        common::wait_for(Duration::from_secs(3), || harness.mocks.a.count()
            > a_after_first)
        .await,
        "到期 provider 应被探针命中"
    );
    let probe = harness.mocks.a.last_request().expect("探针请求");
    assert_eq!(
        probe.json().get("max_tokens").and_then(|v| v.as_u64()),
        Some(1),
        "到期后的首次命中必须是探针，而不是正式请求"
    );
}
