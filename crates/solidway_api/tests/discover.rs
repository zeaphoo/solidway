//! 模型清单获取（`POST /api/models/discover`）的凭据解析。
//!
//! 覆盖：请求凭据优先、编辑态回落已存凭据（base_url 取表单草稿值）、
//! 跨用户引用 `404` 且不发上游调用、无凭据不发请求。

mod common;

use common::{ClientRequest, ClientResponse, Harness, MockResponse};
use serde_json::{Value, json};

fn model_list() -> MockResponse {
    MockResponse::new(200, r#"{"data":[{"id":"gpt-5"},{"id":"gpt-5-mini"}]}"#)
        .with_header("content-type", "application/json")
}

async fn discover(harness: &Harness, cookie: &str, body: Value) -> ClientResponse {
    harness
        .client
        .send(ClientRequest::post_json("/api/models/discover", &body).header("cookie", cookie))
        .await
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

async fn create_user(harness: &Harness, admin_cookie: &str, username: &str) {
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
}

#[tokio::test]
async fn edit_mode_uses_saved_credential_with_form_base_url() {
    let harness = Harness::two_providers("discover-saved", "").await;
    let cookie = harness.admin_cookie().await;
    // 表单草稿 base_url 指向 B，凭据引用 A：结果必须「B 的地址 + A 的已存 key」。
    harness.mocks.b.script_one(model_list());

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "base_url": harness.mocks.b.base_url(),
            "api_key": "",
            "provider_key_id": "p-a"
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!(["gpt-5", "gpt-5-mini"]));
    assert!(body["error"].is_null(), "{body}");

    assert_eq!(harness.mocks.a.count(), 0, "不得回头用已存记录的 base_url");
    assert_eq!(harness.mocks.b.count(), 1);
    let request = harness.mocks.b.last_request().expect("request");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/v1/models");
    assert_eq!(
        request.header("authorization"),
        Some("Bearer sk-provider-a"),
        "应使用已保存的 api_key"
    );
}

#[tokio::test]
async fn request_credential_wins_over_saved_one() {
    let harness = Harness::two_providers("discover-request-key", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.a.script_one(model_list());

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "base_url": harness.mocks.a.base_url(),
            "api_key": "sk-request",
            "provider_key_id": "p-a"
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let request = harness.mocks.a.last_request().expect("request");
    assert_eq!(request.header("authorization"), Some("Bearer sk-request"));
}

#[tokio::test]
async fn cross_user_reference_is_not_found_and_never_calls_upstream() {
    let harness = Harness::two_providers("discover-owner", "").await;
    let admin_cookie = harness.admin_cookie().await;
    create_user(&harness, &admin_cookie, "alice").await;
    let alice_cookie = login(&harness, "alice", "user-test-pass").await;
    harness.mocks.a.set_fallback(model_list());

    let response = discover(
        &harness,
        &alice_cookie,
        json!({
            "provider": "openai-custom",
            "base_url": harness.mocks.a.base_url(),
            "api_key": "",
            "provider_key_id": "p-a"
        }),
    )
    .await;

    assert_eq!(response.status, 404, "{}", response.text());
    assert_eq!(harness.mocks.a.count(), 0, "越权引用不得发起上游调用");
}

#[tokio::test]
async fn missing_credential_returns_empty_list_without_upstream_call() {
    let harness = Harness::two_providers("discover-no-key", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.a.set_fallback(model_list());

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "base_url": harness.mocks.a.base_url(),
            "api_key": ""
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert!(
        body["error"].as_str().is_some_and(|text| !text.is_empty()),
        "应返回可读 error：{body}"
    );
    assert_eq!(harness.mocks.a.count(), 0, "无凭据不得发起上游调用");
}
