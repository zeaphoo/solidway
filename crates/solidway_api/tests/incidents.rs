//! 故障记录与 Webhook 告警。

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{ClientRequest, Harness, MockResponse};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TENANT_KEY: &str = "gw-tenant";

fn chat() -> serde_json::Value {
    json!({"model": "gpt-5", "messages": [{"role": "user", "content": "hi"}]})
}

async fn send_chat(harness: &Harness) {
    harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat()).bearer(TENANT_KEY))
        .await;
}

/// 极简 Webhook 接收端：记录收到的请求体，返回 200。
async fn spawn_webhook() -> (String, Arc<Mutex<Vec<String>>>) {
    let (url, delivered, _, _) = spawn_webhook_with_control().await;
    (url, delivered)
}

/// 可控 Webhook：`fail=true` 时返回 503，用于验证发送失败后的重试/回滚。
async fn spawn_webhook_with_control() -> (
    String,
    Arc<Mutex<Vec<String>>>,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let delivered = Arc::new(Mutex::new(Vec::new()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(AtomicBool::new(false));
    let sink = Arc::clone(&delivered);
    let attempt_sink = Arc::clone(&attempts);
    let fail_sink = Arc::clone(&fail);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let sink = Arc::clone(&sink);
            let attempt_sink = Arc::clone(&attempt_sink);
            let fail_sink = Arc::clone(&fail_sink);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 64 * 1024];
                let mut received = Vec::new();
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            received.extend_from_slice(&buf[..n]);
                            // 读到一个完整的 JSON 体（简单判定：包含右花括号）就停止。
                            if received.windows(1).any(|w| w == b"}") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let text = String::from_utf8_lossy(&received).to_string();
                attempt_sink.fetch_add(1, Ordering::Relaxed);
                let response: &[u8] = if fail_sink.load(Ordering::Relaxed) {
                    b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\n\r\n"
                } else {
                    sink.lock().unwrap().push(text);
                    b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n"
                };
                let _ = stream.write_all(response).await;
            });
        }
    });
    (format!("http://{addr}/hook"), delivered, attempts, fail)
}

/// 允许的登录页以外的管理接口：设置告警。
async fn set_alert(harness: &Harness, cookie: &str, body: serde_json::Value) {
    let response = harness
        .client
        .send(
            ClientRequest::new("PATCH", "/api/settings")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .raw_body(serde_json::to_vec(&json!({ "alert": body })).unwrap()),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
}

async fn incidents(harness: &Harness, cookie: &str) -> Vec<serde_json::Value> {
    for _ in 0..50 {
        let response = harness
            .client
            .send(ClientRequest::new("GET", "/api/provider-incidents").header("cookie", cookie))
            .await;
        assert_eq!(response.status, 200, "{}", response.text());
        let rows = response.json().as_array().cloned().unwrap_or_default();
        if !rows.is_empty() {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Vec::new()
}

#[tokio::test]
async fn unauthorized_creates_manual_action_incident() {
    let harness = Harness::two_providers("incident-401", "").await;
    harness.mocks.a.script(vec![MockResponse::unauthorized()]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    send_chat(&harness).await;
    let cookie = harness.admin_cookie().await;
    let rows = incidents(&harness, &cookie).await;
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["kind"], "http_401");
    assert_eq!(rows[0]["severity"], "manual_action");
    assert_eq!(rows[0]["state"], "open");
    assert_eq!(rows[0]["last_status"], 401);
}

#[tokio::test]
async fn manual_clear_closes_incident() {
    let harness = Harness::two_providers("incident-clear", "").await;
    harness.mocks.a.script(vec![MockResponse::unauthorized()]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    send_chat(&harness).await;
    let cookie = harness.admin_cookie().await;
    let rows = incidents(&harness, &cookie).await;
    let provider_id = rows[0]["provider_id"].as_str().unwrap().to_string();

    let response = harness
        .client
        .send(
            ClientRequest::new("DELETE", &format!("/api/provider-health/{provider_id}"))
                .header("cookie", &cookie),
        )
        .await;
    assert_eq!(response.status, 204, "{}", response.text());

    let rows = incidents(&harness, &cookie).await;
    assert_eq!(rows[0]["state"], "closed");
    assert_eq!(rows[0]["resolved_by"], "manual");
    assert!(rows[0]["ended_at"].as_i64().is_some());
}

#[tokio::test]
async fn quota_failure_is_degraded_and_alerts_once() {
    let harness = Harness::two_providers("incident-quota", "").await;
    harness
        .mocks
        .a
        .script(vec![MockResponse::insufficient_quota()]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let (url, bodies) = spawn_webhook().await;
    let cookie = harness.admin_cookie().await;
    set_alert(
        &harness,
        &cookie,
        json!({ "webhook_url": url, "min_interval_minutes": 0 }),
    )
    .await;

    send_chat(&harness).await;

    assert!(
        common::wait_for(Duration::from_secs(5), || !bodies.lock().unwrap().is_empty()).await,
        "degraded 故障应触发一次 Webhook"
    );
    let rows = incidents(&harness, &cookie).await;
    assert_eq!(rows[0]["kind"], "insufficient_quota");
    assert_eq!(rows[0]["severity"], "degraded");
    let payload = bodies.lock().unwrap().join("\n");
    assert!(payload.contains("provider_incident_open"), "{payload}");
    assert!(payload.contains("insufficient_quota"), "{payload}");
    // 不含任何凭据明文。
    assert!(!payload.contains("sk-"), "{payload}");
}

#[tokio::test]
async fn failed_alert_is_retried_after_reopen_and_recovery_is_not_skipped() {
    let harness = Harness::two_providers("incident-alert-retry", "").await;
    let (url, delivered, attempts, fail) = spawn_webhook_with_control().await;
    fail.store(true, Ordering::Relaxed);
    let cookie = harness.admin_cookie().await;
    set_alert(
        &harness,
        &cookie,
        json!({
            "webhook_url": url,
            "min_interval_minutes": 30,
            "notify_recovery": true,
        }),
    )
    .await;

    let state = harness.api.state();
    let pool = state.pool.clone();
    let now = solidway_api::state::now_unix();
    let (id, created) = solidway_api::db::incidents::record(
        &pool,
        "p-a",
        "r-main",
        "http_403",
        now,
        Some(403),
        None,
    )
    .await
    .expect("record incident");
    assert!(created);

    // 首次开告警：三次重试全部失败（1/4/9s 退避）。
    solidway_api::alerts::notify_opened(state, &id).await;
    assert!(
        common::wait_for(Duration::from_secs(8), || attempts.load(Ordering::Relaxed) >= 3).await,
        "应完成三次投递尝试"
    );
    // 等最后一次退避结束，让失败分支回滚去抖键。
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(
        delivered.lock().unwrap().is_empty(),
        "失败期间不应有成功投递"
    );

    // Webhook 恢复后关闭故障：开告警失败不得连带跳过恢复通知。
    fail.store(false, Ordering::Relaxed);
    let closed = solidway_api::db::incidents::close_open(&pool, "p-a", now + 1, "manual")
        .await
        .expect("close incident");
    assert_eq!(closed, vec![id]);
    solidway_api::alerts::notify_closed(state, &closed).await;
    assert!(
        common::wait_for(Duration::from_secs(5), || {
            delivered
                .lock()
                .unwrap()
                .iter()
                .any(|body| body.contains("provider_incident_closed"))
        })
        .await,
        "开告警发送失败后，恢复通知仍必须发出"
    );

    // 同故障再开：去抖键已在失败时回滚，必须再次告警。
    let (reopened, created) = solidway_api::db::incidents::record(
        &pool,
        "p-a",
        "r-main",
        "http_403",
        now + 2,
        Some(403),
        None,
    )
    .await
    .expect("reopen incident");
    assert!(created, "旧记录已关闭，同故障应新开");
    solidway_api::alerts::notify_opened(state, &reopened).await;
    assert!(
        common::wait_for(Duration::from_secs(5), || {
            delivered
                .lock()
                .unwrap()
                .iter()
                .any(|body| body.contains("provider_incident_open"))
        })
        .await,
        "发送失败回滚去抖键后，同故障再开仍应告警"
    );
}

#[tokio::test]
async fn transient_failure_does_not_alert() {
    let harness = Harness::two_providers("incident-5xx", "").await;
    harness.mocks.a.script(vec![MockResponse::overloaded()]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let (url, bodies) = spawn_webhook().await;
    let cookie = harness.admin_cookie().await;
    set_alert(&harness, &cookie, json!({ "webhook_url": url })).await;

    send_chat(&harness).await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        bodies.lock().unwrap().is_empty(),
        "transient 故障不应触发告警：{:?}",
        bodies.lock().unwrap()
    );
    // 但故障记录仍然留下。
    let rows = incidents(&harness, &cookie).await;
    assert_eq!(rows[0]["severity"], "transient");
}
