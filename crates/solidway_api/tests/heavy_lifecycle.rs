//! 重资源生命周期：Route 归档/恢复/转移、User 归档/offboard、历史归属与墓碑。

mod common;

use std::time::Duration;

use common::{ClientRequest, Harness, TENANT_KEY};
use serde_json::json;

fn chat_request() -> serde_json::Value {
    json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "hello"}],
        "stream": false
    })
}

async fn login(harness: &Harness, username: &str, password: &str) -> String {
    let response = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({ "username": username, "password": password }),
        ))
        .await;
    assert_eq!(response.status, 200, "login failed: {}", response.text());
    response
        .header("set-cookie")
        .expect("set-cookie")
        .split(';')
        .next()
        .expect("cookie")
        .to_string()
}

async fn create_user(harness: &Harness, admin_cookie: &str, username: &str) -> String {
    let response = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/users",
                &json!({
                    "username": username,
                    "password": "user-test-pass",
                    "role": "user"
                }),
            )
            .header("cookie", admin_cookie),
        )
        .await;
    assert_eq!(response.status, 201, "{}", response.text());
    response.json()["id"].as_str().expect("user id").to_string()
}

async fn enable_all_details(harness: &Harness, admin_id: &str) {
    let state = harness.api.state();
    let mut usage = solidway_api::settings::load_usage(&state.pool)
        .await
        .expect("load usage settings");
    usage.detail_mode = solidway_api::settings::DetailMode::All;
    usage.detail_sample_rate = 1.0;
    solidway_api::settings::save_usage(
        &state.pool,
        &usage,
        admin_id,
        solidway_api::state::now_unix(),
    )
    .await
    .expect("save usage settings");
    state.refresh_usage_settings().await;
}

async fn wait_for_usage_owner(harness: &Harness, owner_id: &str, min_requests: i64) -> i64 {
    for _ in 0..60 {
        let total: Option<i64> =
            sqlx::query_scalar("SELECT SUM(requests) FROM usage_bucket WHERE owner_id = ?1")
                .bind(owner_id)
                .fetch_one(&harness.api.state().pool)
                .await
                .expect("sum usage");
        if total.unwrap_or_default() >= min_requests {
            return total.unwrap_or_default();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    0
}

#[tokio::test]
async fn route_archive_releases_name_and_destroys_children() {
    let harness = Harness::two_providers("heavy-archive", "").await;
    harness
        .mocks
        .a
        .set_fallback(common::MockResponse::ok("gpt-5"));
    harness
        .mocks
        .b
        .set_fallback(common::MockResponse::ok("gpt-5"));
    let cookie = harness.admin_cookie().await;
    let admin_id: String = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", &cookie))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    enable_all_details(&harness, &admin_id).await;

    let response = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat_request()).bearer(TENANT_KEY))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());

    let archived = harness
        .client
        .send(ClientRequest::new("POST", "/api/routes/r-main/archive").header("cookie", &cookie))
        .await;
    assert_eq!(archived.status, 204, "{}", archived.text());

    // 默认列表隐藏；include_archived 可见。
    let active = harness
        .client
        .send(ClientRequest::new("GET", "/api/routes").header("cookie", &cookie))
        .await
        .json();
    assert!(active.as_array().unwrap().is_empty(), "{active}");

    let all = harness
        .client
        .send(
            ClientRequest::new("GET", "/api/routes?include_archived=true")
                .header("cookie", &cookie),
        )
        .await
        .json();
    assert_eq!(all[0]["id"], "r-main");
    assert!(all[0]["archived_at"].is_number(), "{all}");

    // 归档后数据面不再加载该路由。
    let blocked = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat_request()).bearer(TENANT_KEY))
        .await;
    assert_eq!(blocked.status, 401, "{}", blocked.text());

    // 子资源活行被删除，墓碑保留身份。
    let providers = harness
        .client
        .send(
            ClientRequest::new("GET", "/api/routes/r-main/provider-keys").header("cookie", &cookie),
        )
        .await;
    assert_eq!(providers.status, 200, "{}", providers.text());
    assert!(providers.json().as_array().unwrap().is_empty());
    let provider_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM provider_keys WHERE route_id = 'r-main'")
            .fetch_one(&harness.api.state().pool)
            .await
            .unwrap();
    let key_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM api_keys WHERE route_id = 'r-main'")
            .fetch_one(&harness.api.state().pool)
            .await
            .unwrap();
    let tombstone_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM resource_tombstone WHERE parent_id = 'r-main'",
    )
    .fetch_one(&harness.api.state().pool)
    .await
    .unwrap();
    assert_eq!(provider_count, 0);
    assert_eq!(key_count, 0);
    assert_eq!(tombstone_count, 3);

    // 历史用量仍归属 owner，并在明细中标记 archived。
    assert!(
        wait_for_usage_owner(&harness, &admin_id, 1).await >= 1,
        "归档后历史用量应仍归 owner"
    );

    let mut details = serde_json::Value::Null;
    for _ in 0..60 {
        details = harness
            .client
            .send(ClientRequest::new("GET", "/api/usage/requests").header("cookie", &cookie))
            .await
            .json();
        if details.as_array().is_some_and(|rows| !rows.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let row = details
        .as_array()
        .and_then(|rows| rows.first())
        .expect("detail row");
    assert_eq!(row["route_name"], "main", "{row}");
    assert_eq!(row["route_archived"], true, "{row}");
    assert_eq!(row["provider_deleted"], true, "{row}");

    // 归档不可恢复。
    let restored = harness
        .client
        .send(ClientRequest::new("POST", "/api/routes/r-main/restore").header("cookie", &cookie))
        .await;
    assert_eq!(restored.status, 404, "{}", restored.text());

    // 名称立即释放：可以新建同名活跃路由。
    let created = harness
        .client
        .send(
            ClientRequest::post_json("/api/routes", &json!({ "name": "main" }))
                .header("cookie", &cookie),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    assert_ne!(created.json()["id"], "r-main");

    // 旧 API Key 永久失效。
    let response = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat_request()).bearer(TENANT_KEY))
        .await;
    assert_eq!(response.status, 401, "{}", response.text());
}

#[tokio::test]
async fn route_transfer_does_not_rewrite_history() {
    let harness = Harness::two_providers("heavy-transfer", "").await;
    harness
        .mocks
        .a
        .set_fallback(common::MockResponse::ok("gpt-5"));
    harness
        .mocks
        .b
        .set_fallback(common::MockResponse::ok("gpt-5"));
    let admin_cookie = harness.admin_cookie().await;
    let alice_id = create_user(&harness, &admin_cookie, "alice").await;
    let alice_cookie = login(&harness, "alice", "user-test-pass").await;
    let admin_id = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", &admin_cookie))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_string();

    // 转移前：用量归 admin。
    let response = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat_request()).bearer(TENANT_KEY))
        .await;
    assert_eq!(response.status, 200);
    assert!(wait_for_usage_owner(&harness, &admin_id, 1).await >= 1);

    // 转移给 alice。
    let transferred = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/routes/r-main/transfer",
                &json!({ "owner_id": alice_id }),
            )
            .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(transferred.status, 200, "{}", transferred.text());

    // 原 Key 继续可用；admin 不再能看到路由，alice 可以。
    let response = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat_request()).bearer(TENANT_KEY))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let admin_routes = harness
        .client
        .send(ClientRequest::new("GET", "/api/routes").header("cookie", &admin_cookie))
        .await
        .json();
    assert!(
        admin_routes.as_array().unwrap().is_empty(),
        "{admin_routes}"
    );
    let alice_routes = harness
        .client
        .send(ClientRequest::new("GET", "/api/routes").header("cookie", &alice_cookie))
        .await
        .json();
    assert_eq!(alice_routes[0]["id"], "r-main");

    // 转移后的用量归 alice；admin 的历史不减少。
    assert!(wait_for_usage_owner(&harness, &alice_id, 1).await >= 1);
    assert!(wait_for_usage_owner(&harness, &admin_id, 1).await >= 1);
}

#[tokio::test]
async fn user_archive_and_offboard_require_route_handling() {
    let harness = Harness::two_providers("heavy-user", "").await;
    let admin_cookie = harness.admin_cookie().await;
    let alice_id = create_user(&harness, &admin_cookie, "alice").await;
    let alice_cookie = login(&harness, "alice", "user-test-pass").await;
    let bob_id = create_user(&harness, &admin_cookie, "bob").await;

    let created = harness
        .client
        .send(
            ClientRequest::post_json("/api/routes", &json!({ "name": "alice-route" }))
                .header("cookie", &alice_cookie),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let route_id = created.json()["id"].as_str().unwrap().to_string();

    // 仍有活跃路由：归档与 offboard（无继任者）均拒绝。
    let archived = harness
        .client
        .send(
            ClientRequest::new("POST", &format!("/api/users/{alice_id}/archive"))
                .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(archived.status, 409, "{}", archived.text());

    let offboard_missing = harness
        .client
        .send(
            ClientRequest::post_json(&format!("/api/users/{alice_id}/offboard"), &json!({}))
                .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(offboard_missing.status, 409, "{}", offboard_missing.text());

    // offboard 指定继任者：转移路由 + 归档 + 撤销会话。
    let offboard = harness
        .client
        .send(
            ClientRequest::post_json(
                &format!("/api/users/{alice_id}/offboard"),
                &json!({ "transfer_to": bob_id }),
            )
            .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(offboard.status, 204, "{}", offboard.text());

    let old_session = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", &alice_cookie))
        .await;
    assert_eq!(old_session.status, 401);

    let relogin = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({ "username": "alice", "password": "user-test-pass" }),
        ))
        .await;
    assert_eq!(relogin.status, 401);

    let restored = harness
        .client
        .send(
            ClientRequest::new("POST", &format!("/api/users/{alice_id}/restore"))
                .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(restored.status, 404, "{}", restored.text());

    let bob_cookie = login(&harness, "bob", "user-test-pass").await;
    let bob_routes = harness
        .client
        .send(ClientRequest::new("GET", "/api/routes").header("cookie", &bob_cookie))
        .await
        .json();
    assert_eq!(bob_routes[0]["id"], route_id.as_str());
}

#[tokio::test]
async fn last_active_admin_cannot_be_disabled_or_archived() {
    let harness = Harness::two_providers("heavy-last-admin", "").await;
    let cookie = harness.admin_cookie().await;
    let admin_id = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", &cookie))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let disabled = harness
        .client
        .send(
            ClientRequest::new("PATCH", &format!("/api/users/{admin_id}"))
                .header("content-type", "application/json")
                .raw_body(serde_json::to_vec(&json!({ "disabled": true })).unwrap())
                .header("cookie", &cookie),
        )
        .await;
    assert_eq!(disabled.status, 409, "{}", disabled.text());

    let archived = harness
        .client
        .send(
            ClientRequest::new("POST", &format!("/api/users/{admin_id}/archive"))
                .header("cookie", &cookie),
        )
        .await;
    assert_eq!(archived.status, 409, "{}", archived.text());
}

#[tokio::test]
async fn deleting_provider_keeps_history_label_from_tombstone() {
    let harness = Harness::two_providers("heavy-tombstone", "").await;
    harness
        .mocks
        .a
        .set_fallback(common::MockResponse::ok("gpt-5"));
    harness
        .mocks
        .b
        .set_fallback(common::MockResponse::ok("gpt-5"));
    let cookie = harness.admin_cookie().await;
    let admin_id: String = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", &cookie))
        .await
        .json()["id"]
        .as_str()
        .unwrap()
        .to_string();
    enable_all_details(&harness, &admin_id).await;

    let response = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat_request()).bearer(TENANT_KEY))
        .await;
    assert_eq!(response.status, 200);

    // 等明细落库后再删 Provider。
    for _ in 0..60 {
        let rows = harness
            .client
            .send(ClientRequest::new("GET", "/api/usage/requests").header("cookie", &cookie))
            .await
            .json();
        if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let deleted = harness
        .client
        .send(ClientRequest::new("DELETE", "/api/provider-keys/p-a").header("cookie", &cookie))
        .await;
    assert_eq!(deleted.status, 204, "{}", deleted.text());

    let tombstone: Option<(String,)> = sqlx::query_as(
        "SELECT label FROM resource_tombstone WHERE kind = 'provider_key' AND resource_id = 'p-a'",
    )
    .fetch_optional(&harness.api.state().pool)
    .await
    .expect("query tombstone");
    assert_eq!(tombstone.map(|row| row.0).as_deref(), Some("p-a"));

    let rows = harness
        .client
        .send(ClientRequest::new("GET", "/api/usage/requests").header("cookie", &cookie))
        .await
        .json();
    let row = rows.as_array().and_then(|rows| rows.first()).expect("row");
    assert_eq!(row["provider_name"], "p-a", "{row}");
    assert_eq!(row["provider_deleted"], true, "{row}");
}

#[tokio::test]
async fn admin_can_close_orphan_incident() {
    let harness = Harness::two_providers("heavy-incident", "").await;
    let cookie = harness.admin_cookie().await;
    let now = solidway_api::state::now_unix();
    sqlx::query(
        "INSERT INTO provider_incident
            (id, provider_id, route_id, owner_id, kind, severity, state,
             started_at, last_seen_at, attempts)
         VALUES ('i-orphan', 'p-gone', 'r-gone', '', 'http_401', 'manual_action', 'open', ?1, ?1, 1)",
    )
    .bind(now)
    .execute(&harness.api.state().pool)
    .await
    .expect("insert orphan incident");

    let closed = harness
        .client
        .send(
            ClientRequest::new("POST", "/api/admin/provider-incidents/i-orphan/close")
                .header("cookie", &cookie),
        )
        .await;
    assert_eq!(closed.status, 204, "{}", closed.text());
}


#[tokio::test]
async fn archiving_route_closes_open_incidents() {
    let harness = Harness::two_providers("heavy-archive-incident", "").await;
    harness.mocks.a.script(vec![common::MockResponse::unauthorized()]);
    harness.mocks
        .b
        .set_fallback(common::MockResponse::ok("gpt-5"));
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(ClientRequest::post_json("/v1/chat/completions", &chat_request()).bearer(TENANT_KEY))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());

    let mut opened = false;
    for _ in 0..50 {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM provider_incident WHERE state = 'open'",
        )
        .fetch_one(&harness.api.state().pool)
        .await
        .expect("count incidents");
        if count > 0 {
            opened = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(opened, "401 应产生 open incident");

    let archived = harness
        .client
        .send(
            ClientRequest::new("POST", "/api/routes/r-main/archive")
                .header("cookie", &cookie),
        )
        .await;
    assert_eq!(archived.status, 204, "{}", archived.text());

    let (open, resolved): (i64, Option<String>) = sqlx::query_as(
        "SELECT COUNT(*), MAX(resolved_by) FROM provider_incident WHERE state = 'open'",
    )
    .fetch_one(&harness.api.state().pool)
    .await
    .expect("query incidents");
    assert_eq!(open, 0, "归档后不应再有 open incident");
    assert_eq!(resolved.as_deref(), None);
    let route_archived: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM provider_incident WHERE resolved_by = 'route_archived'",
    )
    .fetch_one(&harness.api.state().pool)
    .await
    .expect("query resolved");
    assert!(route_archived > 0);
}

#[tokio::test]
async fn transfer_rejects_target_owner_name_conflict() {
    let harness = Harness::two_providers("heavy-transfer-conflict", "").await;
    let admin_cookie = harness.admin_cookie().await;
    let alice_id = create_user(&harness, &admin_cookie, "alice").await;
    let alice_cookie = login(&harness, "alice", "user-test-pass").await;

    // alice 先创建同名活跃路由。
    let created = harness
        .client
        .send(
            ClientRequest::post_json("/api/routes", &json!({ "name": "main" }))
                .header("cookie", &alice_cookie),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());

    let transferred = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/routes/r-main/transfer",
                &json!({ "owner_id": alice_id }),
            )
            .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(transferred.status, 409, "{}", transferred.text());
}
