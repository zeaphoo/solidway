//! 桌面自动登录：令牌门禁、模式门禁、无口令 admin、令牌授权设置口令与进程内复用。

mod common;

use common::{ClientRequest, Harness, MockResponse, SeedRoute};
use serde_json::Value;
use solidway_api::RunMode;

const GATEWAY_KEY: &str = "gw-tenant";

async fn harness(tag: &str, mode: RunMode) -> Harness {
    let harness = common::build_harness_with_mode(tag, "", "/v1", mode, |mocks| {
        vec![
            SeedRoute::new("r-main", "main")
                .provider("p-a", mocks.a.base_url(), &["gpt-5"])
                .key("k-main", GATEWAY_KEY),
        ]
    })
    .await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness
}

fn json(response: &common::ClientResponse) -> Value {
    serde_json::from_str(&response.text()).expect("响应应为 JSON")
}

fn login_body(token: &str) -> Value {
    serde_json::json!({ "token": token })
}

fn cookie_of(response: &common::ClientResponse) -> String {
    response
        .header("set-cookie")
        .expect("应有 set-cookie")
        .split(';')
        .next()
        .expect("cookie 段")
        .to_string()
}

#[tokio::test]
async fn desktop_token_exchanges_for_admin_session() {
    let harness = harness("desktop-login-ok", RunMode::Desktop).await;
    let token = harness
        .api
        .desktop_login_token()
        .expect("desktop 模式应签发令牌")
        .to_string();

    let response = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/desktop",
            &login_body(&token),
        ))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let cookie = cookie_of(&response);
    assert_eq!(json(&response)["role"], "admin");

    let me = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", &cookie))
        .await;
    assert_eq!(me.status, 200, "{}", me.text());
    let body = json(&me);
    assert_eq!(body["username"], "admin");
    assert_eq!(body["role"], "admin");
}

#[tokio::test]
async fn token_is_reusable_within_process() {
    let harness = harness("desktop-login-reuse", RunMode::Desktop).await;
    let token = harness
        .api
        .desktop_login_token()
        .expect("desktop token")
        .to_string();

    for _ in 0..2 {
        let response = harness
            .client
            .send(ClientRequest::post_json(
                "/api/auth/desktop",
                &login_body(&token),
            ))
            .await;
        assert_eq!(response.status, 200, "{}", response.text());
    }
}

#[tokio::test]
async fn wrong_token_is_rejected() {
    let harness = harness("desktop-login-wrong", RunMode::Desktop).await;

    let response = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/desktop",
            &login_body("not-a-real-token"),
        ))
        .await;
    assert_eq!(response.status, 401, "{}", response.text());
    assert_eq!(
        json(&response)["error"]["code"],
        "desktop_login_unavailable"
    );
}

#[tokio::test]
async fn standalone_has_no_desktop_login() {
    let harness = harness("desktop-login-standalone", RunMode::Standalone).await;
    assert!(harness.api.desktop_login_token().is_none());

    let response = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/desktop",
            &login_body("whatever"),
        ))
        .await;
    assert_eq!(response.status, 401, "{}", response.text());
    assert_eq!(
        json(&response)["error"]["code"],
        "desktop_login_unavailable"
    );
}

/// 把 admin 打回「未设置口令」，模拟桌面首启（不再有引导令牌）。
async fn lock_admin(harness: &Harness) {
    sqlx::query("UPDATE users SET password_hash = '!' WHERE role = 'admin'")
        .execute(&harness.api.state().pool)
        .await
        .expect("lock admin");
}

#[tokio::test]
async fn desktop_mode_does_not_issue_bootstrap_token() {
    let desktop = harness("desktop-login-no-bootstrap", RunMode::Desktop).await;
    assert!(
        desktop.api.bootstrap_token().is_none(),
        "桌面模式不得签发引导令牌"
    );
    assert!(
        desktop.api.desktop_login_token().is_some(),
        "桌面模式应签发登录令牌"
    );

    let standalone = harness("desktop-login-standalone-bootstrap", RunMode::Standalone).await;
    let token = standalone
        .api
        .bootstrap_token()
        .expect("standalone 应签发引导令牌");
    assert!(token.len() >= 32, "引导令牌至少 32 字节");
    assert!(standalone.api.desktop_login_token().is_none());
}

#[tokio::test]
async fn admin_without_password_can_auto_login() {
    let harness = harness("desktop-login-locked", RunMode::Desktop).await;
    let token = harness
        .api
        .desktop_login_token()
        .expect("desktop token")
        .to_string();

    lock_admin(&harness).await;

    // 授权依据是令牌而非口令：未设置口令不影响免登录（ADR 0017）。
    let response = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/desktop",
            &login_body(&token),
        ))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(json(&response)["has_password"], false);

    // fail-closed：无口令 = 无法口令登录。
    let login = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &serde_json::json!({"username": "admin", "password": "whatever"}),
        ))
        .await;
    assert_eq!(login.status, 401, "{}", login.text());
}

#[tokio::test]
async fn desktop_token_sets_password_without_current_password() {
    let harness = harness("desktop-login-set-password", RunMode::Desktop).await;
    let token = harness
        .api
        .desktop_login_token()
        .expect("desktop token")
        .to_string();
    lock_admin(&harness).await;

    let session = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/desktop",
            &login_body(&token),
        ))
        .await;
    assert_eq!(session.status, 200, "{}", session.text());
    let cookie = cookie_of(&session);

    let changed = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/auth/password",
                &serde_json::json!({
                    "new_password": "desktop-set-pass",
                    "desktop_token": token,
                }),
            )
            .header("cookie", &cookie),
        )
        .await;
    assert_eq!(changed.status, 204, "{}", changed.text());

    let login = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &serde_json::json!({"username": "admin", "password": "desktop-set-pass"}),
        ))
        .await;
    assert_eq!(login.status, 200, "{}", login.text());

    let me = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", &cookie))
        .await;
    assert_eq!(me.status, 200, "{}", me.text());
    assert_eq!(json(&me)["has_password"], true);
}

#[tokio::test]
async fn password_change_without_token_requires_current_password() {
    let harness = harness("desktop-login-need-current", RunMode::Desktop).await;
    let token = harness
        .api
        .desktop_login_token()
        .expect("desktop token")
        .to_string();
    lock_admin(&harness).await;

    let session = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/desktop",
            &login_body(&token),
        ))
        .await;
    let cookie = cookie_of(&session);

    for body in [
        serde_json::json!({"new_password": "no-current-pass"}),
        serde_json::json!({"new_password": "no-current-pass", "desktop_token": "wrong-token"}),
    ] {
        let response = harness
            .client
            .send(
                ClientRequest::post_json("/api/auth/password", &body).header("cookie", &cookie),
            )
            .await;
        assert_eq!(response.status, 422, "{}", response.text());
    }

    // 口令未被改动：仍然无法口令登录。
    let login = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &serde_json::json!({"username": "admin", "password": "no-current-pass"}),
        ))
        .await;
    assert_eq!(login.status, 401, "{}", login.text());
}

#[tokio::test]
async fn standalone_ignores_desktop_token() {
    let harness = harness("desktop-login-standalone-token", RunMode::Standalone).await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/auth/password",
                &serde_json::json!({
                    "new_password": "standalone-pass",
                    "desktop_token": "whatever",
                }),
            )
            .header("cookie", &cookie),
        )
        .await;
    assert_eq!(response.status, 422, "{}", response.text());
}

#[tokio::test]
async fn non_admin_cannot_use_desktop_token() {
    let harness = harness("desktop-login-staff-token", RunMode::Desktop).await;
    let token = harness
        .api
        .desktop_login_token()
        .expect("desktop token")
        .to_string();
    let admin_cookie = harness.admin_cookie().await;

    let created = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/users",
                &serde_json::json!({
                    "username": "staff",
                    "password": "staff-initial-pass",
                    "role": "user",
                }),
            )
            .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());

    let login = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &serde_json::json!({"username": "staff", "password": "staff-initial-pass"}),
        ))
        .await;
    assert_eq!(login.status, 200, "{}", login.text());
    let staff_cookie = cookie_of(&login);

    let response = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/auth/password",
                &serde_json::json!({
                    "new_password": "staff-no-current",
                    "desktop_token": token,
                }),
            )
            .header("cookie", &staff_cookie),
        )
        .await;
    assert_eq!(response.status, 422, "{}", response.text());
}

#[tokio::test]
async fn desktop_status_is_not_probeable() {
    let harness = harness("desktop-login-probe", RunMode::Desktop).await;
    let response = harness
        .client
        .send(ClientRequest::new("GET", "/api/auth/desktop"))
        .await;
    // 只注册了 POST：GET 必须是 405，且不泄露任何模式/凭据信息。
    assert_eq!(response.status, 405, "{}", response.text());
}
