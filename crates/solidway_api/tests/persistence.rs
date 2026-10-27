//! 健康状态持久化与端到端场景测试（tasks 5.3）。
//!
//! 「重启」在进程内等价为：用同一份配置与同一个 SQLite 文件重新构建网关。

mod common;

use std::time::Duration;

use common::{ClientRequest, Harness, MockResponse};
use serde_json::json;

const TENANT_KEY: &str = "gw-tenant";

fn chat(session: Option<&str>) -> ClientRequest {
    let mut body = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "hello"}]
    });
    let path = match session {
        Some(session) => {
            body["previous_response_id"] = json!(session);
            "/v1/responses"
        }
        None => "/v1/chat/completions",
    };
    ClientRequest::post_json(path, &body).bearer(TENANT_KEY)
}

// ---------------------------------------------------------------------------
// 全新库自动迁移建表（初始化迁移）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fresh_database_is_migrated_automatically() {
    let harness = common::build_harness("fresh", "", "/v1", |_mocks| {
        vec![common::SeedRoute::new("r-main", "main").key("k-main", TENANT_KEY)]
    })
    .await;

    assert!(harness.db_path.exists(), "启动后库文件应被创建");
    let rows = common::health_rows(&harness.db_path).await;
    assert!(rows.is_empty(), "新库健康表应为空表（可查询即迁移成功）");
}

// ---------------------------------------------------------------------------
// 退役与冷却跨重启
// ---------------------------------------------------------------------------

#[tokio::test]
async fn permanent_retirement_survives_restart() {
    let harness = Harness::two_providers("restart-retired", "").await;
    // A 永久退役（余额不足），B 保持健康。
    harness
        .mocks
        .a
        .set_fallback(MockResponse::unauthorized());
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(chat(None)).await;
    assert_eq!(first.status, 200, "B 承载成功");
    assert_eq!(harness.mocks.a.count(), 1, "A 应被尝试过一次");
    let rows = common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| rows.len() == 1).await;
    assert_eq!(rows.len(), 1, "A 应落库为退役：{rows:?}");
    assert_eq!(rows[0].0, "p-a");
    assert_eq!(rows[0].1, "retired");

    // 重启：同一份库与配置。
    let (client, _gateway) = harness.restart("").await;
    let a_count_before = harness.mocks.a.count();
    let b_count_before = harness.mocks.b.count();
    for _ in 0..2 {
        let response = client.send(chat(None)).await;
        assert_eq!(response.status, 200, "重启后健康 provider 仍可承载");
    }
    assert_eq!(
        harness.mocks.a.count(),
        a_count_before,
        "永久退役必须跨重启仍生效（不得被选号）"
    );
    assert_eq!(harness.mocks.b.count(), b_count_before + 2);
}

#[tokio::test]
async fn active_cooldown_keeps_its_deadline_across_restart() {
    let harness = Harness::two_providers("restart-cooldown", "").await;
    // A 返回 429（默认冷却 5m，重启后不会到期）。
    harness
        .mocks
        .a
        .set_fallback(MockResponse::rate_limited(None));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(chat(None)).await;
    assert_eq!(first.status, 200);
    let rows_before = common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| rows.len() == 1).await;
    assert_eq!(rows_before.len(), 1);
    assert_eq!(rows_before[0].1, "quarantined");
    let deadline_before = rows_before[0].2.expect("冷却必须有截止时间");

    let (client, _gateway) = harness.restart("").await;
    let rows_after = common::health_rows(&harness.db_path).await;
    assert_eq!(
        rows_after[0].2,
        Some(deadline_before),
        "冷却截止时间必须跨重启保持不变"
    );

    let a_count_before = harness.mocks.a.count();
    for _ in 0..2 {
        let response = client.send(chat(None)).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(
        harness.mocks.a.count(),
        a_count_before,
        "未到期的冷却跨重启后仍不得参与选号"
    );
}

#[tokio::test]
async fn manual_row_deletion_plus_restart_restores_provider() {
    let harness = Harness::two_providers("manual-restore", "").await;
    harness
        .mocks
        .a
        .set_fallback(MockResponse::unauthorized());
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let first = harness.client.send(chat(None)).await;
    assert_eq!(first.status, 200);
    assert_eq!(harness.mocks.a.count(), 1);
    // 等落库完成再删，避免「删除先于写入」的竞态。
    common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| rows.len() == 1).await;

    // 人工恢复：直接删除库中的退役标记 + 重启（account-health「永久退役的人工恢复」）。
    common::exec_sql(
        &harness.db_path,
        "DELETE FROM provider_health WHERE provider_id = 'p-a'".to_string(),
    )
    .await;
    let rows = common::health_rows(&harness.db_path).await;
    assert!(rows.is_empty(), "人工清除后标记应为空：{rows:?}");

    let (client, _gateway) = harness.restart("").await;
    // A 恢复健康：改用可识别的成功响应，确认它重新参与选号。
    harness
        .mocks
        .a
        .set_fallback(MockResponse::ok("gpt-5").with_header("x-served-by", "p-a"));
    harness
        .mocks
        .b
        .set_fallback(MockResponse::ok("gpt-5").with_header("x-served-by", "p-b"));

    let mut served_by_a = 0;
    for _ in 0..2 {
        let response = client.send(chat(None)).await;
        assert_eq!(response.status, 200);
        if response.header("x-served-by") == Some("p-a") {
            served_by_a += 1;
        }
    }
    assert!(
        served_by_a >= 1,
        "人工清除标记并重启后，provider 应重新参与选号"
    );
}

// ---------------------------------------------------------------------------
// 端到端四场景（可与真实二进制脚本互相印证）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_scenario_failover_is_transparent() {
    let harness = Harness::two_providers("e2e-failover", "").await;
    harness
        .mocks
        .a
        .script(vec![MockResponse::rate_limited(None)]);
    harness.mocks.b.script(vec![MockResponse::ok("gpt-5")]);

    let response = harness.client.send(chat(None)).await;
    assert_eq!(response.status, 200);
    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1);
}

#[tokio::test]
async fn e2e_scenario_same_session_reuses_provider() {
    let harness = Harness::two_providers("e2e-affinity", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    for _ in 0..3 {
        let response = harness.client.send(chat(Some("e2e-sess"))).await;
        assert_eq!(response.status, 200);
    }
    let (sticky, other) = if harness.mocks.a.count() == 3 {
        (&harness.mocks.a, &harness.mocks.b)
    } else {
        (&harness.mocks.b, &harness.mocks.a)
    };
    assert_eq!(sticky.count(), 3, "同会话应复用同一 provider");
    assert_eq!(other.count(), 0);
}

#[tokio::test]
async fn e2e_scenario_retirement_survives_restart() {
    let harness = Harness::two_providers("e2e-retire", "").await;
    harness
        .mocks
        .a
        .set_fallback(MockResponse::unauthorized());
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    harness.client.send(chat(None)).await;
    // 等退役状态落库，再重启（否则重启会读到空的健康表）。
    common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| !rows.is_empty()).await;
    let (client, _gateway) = harness.restart("").await;
    let a_before = harness.mocks.a.count();
    client.send(chat(None)).await;
    assert_eq!(
        harness.mocks.a.count(),
        a_before,
        "退役 provider 重启后仍不参与选号"
    );
}

#[tokio::test]
async fn e2e_scenario_manual_clear_and_restart_recovers() {
    let harness = Harness::two_providers("e2e-manual", "").await;
    harness
        .mocks
        .a
        .set_fallback(MockResponse::unauthorized());
    harness
        .mocks
        .b
        .set_fallback(MockResponse::ok("gpt-5").with_header("x-served-by", "p-b"));

    harness.client.send(chat(None)).await;
    // 等落库完成再清库，避免竞态。
    common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| !rows.is_empty()).await;
    common::exec_sql(&harness.db_path, "DELETE FROM provider_health".to_string()).await;
    let (client, _gateway) = harness.restart("").await;
    harness
        .mocks
        .a
        .set_fallback(MockResponse::ok("gpt-5").with_header("x-served-by", "p-a"));

    let mut recovered = false;
    for _ in 0..2 {
        let response = client.send(chat(None)).await;
        assert_eq!(response.status, 200);
        if response.header("x-served-by") == Some("p-a") {
            recovered = true;
        }
    }
    assert!(recovered, "人工清库 + 重启后 provider 应恢复参与选号");
    let _ = tokio::time::sleep(Duration::from_millis(10)).await;
}
