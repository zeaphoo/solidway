//! 用量统计：token 采集、Provider 维度、普通 / 全局统计接口。

mod common;

use std::time::Duration;

use common::{ClientRequest, Harness, MockResponse};
use serde_json::json;

const TENANT_KEY: &str = "gw-tenant";

fn chat_request(stream: bool) -> serde_json::Value {
    json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": stream
    })
}

fn usage_response() -> MockResponse {
    MockResponse::new(
        200,
        r#"{"id":"chatcmpl-usage","object":"chat.completion","model":"gpt-5","choices":[{"index":0,"message":{"role":"assistant","content":"ok"}}],"usage":{"prompt_tokens":100,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":10},"completion_tokens_details":{"reasoning_tokens":5}}}"#,
    )
    .with_header("content-type", "application/json")
}

fn usage_sse() -> MockResponse {
    MockResponse::sse(
        &[
            "data: {\"id\":\"chatcmpl-usage\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-5\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"}}]}\n\n",
            "data: {\"id\":\"chatcmpl-usage\",\"object\":\"chat.completion.chunk\",\"model\":\"gpt-5\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":20,\"prompt_tokens_details\":{\"cached_tokens\":10},\"completion_tokens_details\":{\"reasoning_tokens\":5}}}\n\n",
            "data: [DONE]\n\n",
        ],
        Duration::ZERO,
    )
}

async fn send_usage(harness: &Harness, stream: bool) {
    let response = harness
        .client
        .send(
            ClientRequest::post_json("/v1/chat/completions", &chat_request(stream))
                .bearer(TENANT_KEY),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
}

async fn wait_flush() {
    tokio::time::sleep(Duration::from_millis(1_500)).await;
}

async fn get_json(harness: &Harness, path: &str, cookie: &str) -> serde_json::Value {
    let response = harness
        .client
        .send(ClientRequest::new("GET", path).header("cookie", cookie))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.json()
}

#[tokio::test]
async fn non_stream_usage_is_aggregated_with_tokens_and_provider() {
    let harness = Harness::two_providers("usage-json", "").await;
    harness.mocks.a.set_fallback(usage_response());
    harness.mocks.b.set_fallback(usage_response());

    send_usage(&harness, false).await;
    wait_flush().await;

    let cookie = harness.admin_cookie().await;
    let config = get_json(&harness, "/api/usage/config", &cookie).await;
    assert_eq!(config["timezone_offset_minutes"], 480);
    let summary = get_json(
        &harness,
        "/api/usage/summary?range=today&tz_offset_minutes=480",
        &cookie,
    )
    .await;
    assert_eq!(summary["requests"], 1);
    assert_eq!(summary["completed"], 1);
    assert_eq!(summary["input_tokens"], 100);
    assert_eq!(summary["output_tokens"], 20);
    assert_eq!(summary["cached_tokens"], 10);
    assert_eq!(summary["reasoning_tokens"], 5);
    assert_eq!(summary["total_tokens"], 120);
    assert_eq!(summary["usage_missing"], 0);

    let providers = get_json(
        &harness,
        "/api/usage/breakdown?range=today&tz_offset_minutes=480&by=provider",
        &cookie,
    )
    .await;
    let rows = providers.as_array().expect("provider rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["input_tokens"], 100);
    assert_eq!(rows[0]["output_tokens"], 20);

    let global = get_json(
        &harness,
        "/api/admin/usage/summary?range=today&tz_offset_minutes=480",
        &cookie,
    )
    .await;
    assert_eq!(global["scope"]["kind"], "global");
    assert_eq!(global["input_tokens"], 100);
}

#[tokio::test]
async fn stream_usage_is_parsed_from_sse() {
    let harness = Harness::two_providers("usage-sse", "").await;
    harness.mocks.a.set_fallback(usage_sse());
    harness.mocks.b.set_fallback(usage_sse());

    send_usage(&harness, true).await;
    wait_flush().await;

    let cookie = harness.admin_cookie().await;
    let summary = get_json(
        &harness,
        "/api/usage/summary?range=today&tz_offset_minutes=480",
        &cookie,
    )
    .await;
    assert_eq!(summary["requests"], 1);
    assert_eq!(summary["completed"], 1);
    assert_eq!(summary["input_tokens"], 100);
    assert_eq!(summary["output_tokens"], 20);
    assert_eq!(summary["usage_missing"], 0);
}

#[tokio::test]
async fn missing_usage_is_counted() {
    let harness = Harness::two_providers("usage-missing", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    send_usage(&harness, false).await;
    wait_flush().await;

    let cookie = harness.admin_cookie().await;
    let summary = get_json(
        &harness,
        "/api/usage/summary?range=today&tz_offset_minutes=480",
        &cookie,
    )
    .await;
    assert_eq!(summary["usage_missing"], 1);
    assert_eq!(summary["input_tokens"], 0);
}

#[tokio::test]
async fn time_presets_and_custom_range_are_accepted() {
    let harness = Harness::two_providers("usage-ranges", "").await;
    let cookie = harness.admin_cookie().await;

    for range in [
        "24h",
        "today",
        "yesterday",
        "7d",
        "30d",
        "this_month",
        "last_month",
    ] {
        let response = harness
            .client
            .send(
                ClientRequest::new(
                    "GET",
                    &format!("/api/usage/summary?range={range}&tz_offset_minutes=480"),
                )
                .header("cookie", &cookie),
            )
            .await;
        assert_eq!(response.status, 200, "range={range}: {}", response.text());
    }

    let now = solidway_api::state::now_unix();
    let response = harness
        .client
        .send(
            ClientRequest::new(
                "GET",
                &format!(
                    "/api/usage/summary?range=custom&from={}&to={}&interval=hour&tz_offset_minutes=480",
                    now - 3_600,
                    now
                ),
            )
            .header("cookie", &cookie),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());

    let invalid = harness
        .client
        .send(ClientRequest::new("GET", "/api/usage/summary?range=1h").header("cookie", &cookie))
        .await;
    assert_eq!(invalid.status, 422);
}

// ---------------------------------------------------------------------------
// 逐条明细（detail_mode = errors）
// ---------------------------------------------------------------------------

/// 轮询直到出现明细（事件泵异步 + 测试并行时可能慢于固定 sleep）。
async fn wait_for_detail(harness: &Harness, cookie: &str) -> Vec<serde_json::Value> {
    for _ in 0..50 {
        let rows = detail_rows(harness, cookie).await;
        if !rows.is_empty() {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Vec::new()
}

/// 读明细接口（默认 24h 窗口）。
async fn detail_rows(harness: &Harness, cookie: &str) -> Vec<serde_json::Value> {
    let response = harness
        .client
        .send(ClientRequest::new("GET", "/api/usage/requests").header("cookie", cookie))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    response.json().as_array().cloned().unwrap_or_default()
}

#[tokio::test]
async fn errors_mode_keeps_failover_detail() {
    let harness = Harness::two_providers("detail-failover", "").await;
    // A 限流 → 换号到 B 成功：最终 200，但发生过换号。
    harness
        .mocks
        .a
        .script(vec![MockResponse::rate_limited(None)]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = harness
        .client
        .send(
            ClientRequest::post_json("/v1/chat/completions", &chat_request(false))
                .bearer(TENANT_KEY),
        )
        .await;
    assert_eq!(response.status, 200);
    let cookie = harness.admin_cookie().await;
    let rows = wait_for_detail(&harness, &cookie).await;
    assert_eq!(rows.len(), 1, "换号请求应留下明细：{rows:?}");
    let row = &rows[0];
    assert_eq!(row["status"], 200);
    assert!(
        row["failovers"].as_i64().unwrap_or(0) >= 1,
        "明细应记录换号次数：{row:?}"
    );
    assert_eq!(row["attempts"], 2, "应记录两次尝试：{row:?}");

    // 每次尝试的明细（provider / status / 延迟）随列表返回。
    let attempts = row["attempts_detail"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(attempts.len(), 2, "应返回两次尝试明细：{row:?}");
    assert_eq!(attempts[0]["status"], 429, "{attempts:?}");
    assert!(
        attempts[0]["provider_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(attempts[0]["latency_ms"].as_i64().is_some());
    assert_eq!(attempts[1]["status"], 200, "{attempts:?}");
}

#[tokio::test]
async fn errors_mode_skips_clean_success() {
    let harness = Harness::two_providers("detail-clean", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = harness
        .client
        .send(
            ClientRequest::post_json("/v1/chat/completions", &chat_request(false))
                .bearer(TENANT_KEY),
        )
        .await;
    assert_eq!(response.status, 200);
    // 反向断言：给事件泵留出足够时间，再确认没有明细。
    tokio::time::sleep(Duration::from_millis(800)).await;

    let cookie = harness.admin_cookie().await;
    let rows = detail_rows(&harness, &cookie).await;
    assert!(rows.is_empty(), "无换号的成功请求不应留明细：{rows:?}");
}

// ---------------------------------------------------------------------------
// 统计时区只读
// ---------------------------------------------------------------------------

#[tokio::test]
async fn timezone_offset_is_read_only() {
    let harness = Harness::two_providers("tz-readonly", "").await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(
            ClientRequest::new("PATCH", "/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(
                    serde_json::to_vec(&json!({ "usage": { "timezone_offset_minutes": 0 } }))
                        .unwrap(),
                ),
        )
        .await;
    assert_eq!(response.status, 422, "{}", response.text());

    // 值保持不变（仍是默认的 480 或环境变量值）。
    let settings = harness
        .client
        .send(ClientRequest::new("GET", "/api/settings").header("cookie", &cookie))
        .await
        .json();
    assert_ne!(settings["usage"]["timezone_offset_minutes"], 0);
}

// ---------------------------------------------------------------------------
// 明细独立库
// ---------------------------------------------------------------------------

#[tokio::test]
async fn details_live_in_separate_database() {
    let harness = Harness::two_providers("detail-store", "").await;
    harness
        .mocks
        .a
        .script(vec![MockResponse::rate_limited(None)]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = harness
        .client
        .send(
            ClientRequest::post_json("/v1/chat/completions", &chat_request(false))
                .bearer(TENANT_KEY),
        )
        .await;
    assert_eq!(response.status, 200);

    let cookie = harness.admin_cookie().await;
    let rows = wait_for_detail(&harness, &cookie).await;
    assert_eq!(rows.len(), 1, "明细应可查询：{rows:?}");

    // 明细库文件存在且真的写入了行。
    let detail_path = harness.data_root.join("usage_detail.db");
    assert!(detail_path.is_file(), "缺少 usage_detail.db");
    let detail_pool = sqlx::SqlitePool::connect(&format!("sqlite:{}", detail_path.display()))
        .await
        .expect("open detail db");
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM usage_request_detail")
        .fetch_one(&detail_pool)
        .await
        .expect("count detail rows");
    assert_eq!(count.0, 1);

    // 主库不再有明细表。
    let main_table: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'usage_request_detail'",
    )
    .fetch_optional(&harness.api.state().pool)
    .await
    .expect("query main schema");
    assert!(main_table.is_none(), "主库不应再有 usage_request_detail");
}

#[tokio::test]
async fn detail_names_are_enriched_and_fall_back_to_ids() {
    let harness = Harness::two_providers("detail-names", "").await;
    harness
        .mocks
        .a
        .script(vec![MockResponse::rate_limited(None)]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = harness
        .client
        .send(
            ClientRequest::post_json("/v1/chat/completions", &chat_request(false))
                .bearer(TENANT_KEY),
        )
        .await;
    assert_eq!(response.status, 200);

    let cookie = harness.admin_cookie().await;
    let rows = wait_for_detail(&harness, &cookie).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    // 路由名 / Key 标签从主库补齐（种子路由名为 main）。
    assert_eq!(row["route_name"], "main", "{row:?}");
    assert!(
        row["api_key_label"]
            .as_str()
            .is_some_and(|label| !label.is_empty())
    );

    // 删除路由后：全局统计（无 owner 过滤）仍能看到明细，名称回退为 null。
    // 普通统计按"当前用户名下的路由"过滤，删除后自然不可见（与 join 版本行为一致）。
    let route_id = row["route_id"].as_str().unwrap().to_string();
    sqlx::query("DELETE FROM routes WHERE id = ?1")
        .bind(&route_id)
        .execute(&harness.api.state().pool)
        .await
        .expect("delete route");

    let admin_rows = harness
        .client
        .send(ClientRequest::new("GET", "/api/admin/usage/requests").header("cookie", &cookie))
        .await;
    assert_eq!(admin_rows.status, 200, "{}", admin_rows.text());
    let admin_rows = admin_rows.json();
    let admin_rows = admin_rows.as_array().cloned().unwrap_or_default();
    assert_eq!(admin_rows.len(), 1, "明细不受主库删除影响：{admin_rows:?}");
    assert!(admin_rows[0]["route_name"].is_null(), "{:?}", admin_rows[0]);
    assert_eq!(admin_rows[0]["route_id"], route_id);
}

/// 把 p-a 改成指定家族格式，并重新下发 core 配置。
async fn switch_provider_family(
    harness: &Harness,
    api_formats: &str,
    auth_scheme: &str,
    base_url: &str,
    model: &str,
) {
    let models =
        serde_json::to_string(&json!([{ "name": model, "upstream": "" }])).expect("models json");
    sqlx::query(
        "UPDATE provider_keys
            SET api_formats = ?1, auth_scheme = ?2, base_url = ?3, models = ?4
          WHERE id = 'p-a'",
    )
    .bind(api_formats)
    .bind(auth_scheme)
    .bind(base_url)
    .bind(models)
    .execute(&harness.api.state().pool)
    .await
    .expect("update provider");
    harness.api.state().sync_core().await.expect("sync core");
}

fn anthropic_usage_sse() -> MockResponse {
    MockResponse::sse(
        &[
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude\",\"usage\":{\"input_tokens\":100,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":10}}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":50}}\n\n",
        ],
        Duration::ZERO,
    )
}

#[tokio::test]
async fn anthropic_stream_usage_is_attributed_to_provider() {
    let harness = Harness::two_providers("usage-anthropic", "").await;
    switch_provider_family(
        &harness,
        r#"["anthropic_messages"]"#,
        "x-api-key",
        &harness.mocks.a.base_url(),
        "claude",
    )
    .await;
    harness.mocks.a.set_fallback(anthropic_usage_sse());

    let response = harness
        .client
        .send(
            ClientRequest::post_json(
                "/v1/messages",
                &json!({
                    "model": "claude",
                    "max_tokens": 8,
                    "stream": true,
                    "metadata": {"user_id": "u-1"}
                }),
            )
            .header("x-api-key", TENANT_KEY),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    wait_flush().await;

    let cookie = harness.admin_cookie().await;
    let summary = get_json(
        &harness,
        "/api/usage/summary?range=today&tz_offset_minutes=480",
        &cookie,
    )
    .await;
    assert_eq!(summary["requests"], 1);
    assert_eq!(summary["input_tokens"], 130);
    assert_eq!(summary["output_tokens"], 50);
    assert_eq!(summary["cached_tokens"], 20);
    assert_eq!(summary["cache_write_tokens"], 10);
    assert_eq!(summary["usage_missing"], 0);

    let breakdown = get_json(
        &harness,
        "/api/usage/breakdown?range=today&tz_offset_minutes=480&by=provider",
        &cookie,
    )
    .await;
    let rows = breakdown.as_array().expect("provider rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["key"], "p-a");
    assert_eq!(rows[0]["input_tokens"], 130);
    assert_eq!(rows[0]["output_tokens"], 50);
}
