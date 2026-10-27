//! 创建 provider 凭据时的模型清单自动拉取（`autodiscover`）。
//!
//! 覆盖：成功填充同名映射、失败不阻断创建、未返回模型按失败、清单非空 /
//! 服务账号凭据 / `PATCH` 不触发、超时按失败且响应延迟有上界。

mod common;

use std::time::{Duration, Instant};

use common::{ClientRequest, ClientResponse, Harness, MockResponse};
use serde_json::{Value, json};

fn model_list(ids: &[&str]) -> MockResponse {
    let data: Vec<Value> = ids.iter().map(|id| json!({ "id": id })).collect();
    MockResponse::new(200, serde_json::to_vec(&json!({ "data": data })).unwrap())
        .with_header("content-type", "application/json")
}

async fn create_provider(harness: &Harness, cookie: &str, body: Value) -> ClientResponse {
    harness
        .client
        .send(
            ClientRequest::post_json("/api/routes/r-main/provider-keys", &body)
                .header("cookie", cookie),
        )
        .await
}

fn assert_no_error(body: &Value) {
    assert!(
        body["autodiscover"].get("error").is_none(),
        "ok / skipped 不得携带 error：{body}"
    );
}

#[tokio::test]
async fn create_fills_same_name_mappings_from_upstream() {
    let harness = Harness::two_providers("autodiscover-ok", "").await;
    let cookie = harness.admin_cookie().await;
    harness
        .mocks
        .c
        .script_one(model_list(&["gpt-5", "gpt-5", "gpt-5-mini"]));

    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-auto-ok",
            "base_url": harness.mocks.c.base_url()
        }),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(
        body["models"],
        json!([
            {"name": "gpt-5", "upstream": "gpt-5"},
            {"name": "gpt-5-mini", "upstream": "gpt-5-mini"}
        ])
    );
    assert_eq!(body["autodiscover"]["status"], "ok");
    assert_no_error(&body);

    // 只发一次上游调用，凭据为该次创建提交的 api_key。
    assert_eq!(harness.mocks.c.count(), 1);
    let request = harness.mocks.c.last_request().expect("request");
    assert_eq!(request.method, "GET");
    assert_eq!(request.path, "/v1/models");
    assert_eq!(request.header("authorization"), Some("Bearer sk-auto-ok"));

    // 列表 / 单条读取响应不携带 autodiscover，模型已落库。
    let listed = harness
        .client
        .send(
            ClientRequest::new("GET", "/api/routes/r-main/provider-keys").header("cookie", &cookie),
        )
        .await
        .json();
    let created = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == body["id"])
        .expect("created provider");
    assert!(created.get("autodiscover").is_none(), "{created}");
    assert_eq!(created["models"], body["models"]);
}

#[tokio::test]
async fn create_succeeds_when_upstream_fails() {
    let harness = Harness::two_providers("autodiscover-fail", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(MockResponse::new(500, "{}"));

    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-auto-fail",
            "base_url": harness.mocks.c.base_url()
        }),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["autodiscover"]["status"], "failed");
    assert!(
        body["autodiscover"]["error"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "failed 必须带可读 error：{body}"
    );

    // 落库仍为空清单，用户可稍后手动补齐。
    let fetched = harness
        .client
        .send(
            ClientRequest::new(
                "GET",
                &format!("/api/provider-keys/{}", body["id"].as_str().unwrap()),
            )
            .header("cookie", &cookie),
        )
        .await
        .json();
    assert_eq!(fetched["models"], json!([]));
}

#[tokio::test]
async fn empty_upstream_list_is_failure_not_ok() {
    let harness = Harness::two_providers("autodiscover-empty", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(model_list(&[]));

    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-auto-empty",
            "base_url": harness.mocks.c.base_url()
        }),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["autodiscover"]["status"], "failed");
    assert!(
        body["autodiscover"]["error"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "{body}"
    );
}

#[tokio::test]
async fn provided_models_skip_autodiscover_without_upstream_call() {
    let harness = Harness::two_providers("autodiscover-skip", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.set_fallback(model_list(&["gpt-9"]));

    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-auto-skip",
            "base_url": harness.mocks.c.base_url(),
            "models": [{"name": "gpt-5"}]
        }),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(
        body["models"],
        json!([{"name": "gpt-5", "upstream": "gpt-5"}])
    );
    assert_eq!(body["autodiscover"]["status"], "skipped");
    assert_no_error(&body);
    assert_eq!(harness.mocks.c.count(), 0, "清单非空不得发起上游调用");
}

#[tokio::test]
async fn whitespace_only_models_count_as_empty_and_trigger() {
    let harness = Harness::two_providers("autodiscover-normalized", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(model_list(&["gpt-5"]));

    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-auto-normalized",
            "base_url": harness.mocks.c.base_url(),
            "models": [{"name": "   "}]
        }),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(
        body["models"],
        json!([{"name": "gpt-5", "upstream": "gpt-5"}])
    );
    assert_eq!(body["autodiscover"]["status"], "ok");
    assert_eq!(harness.mocks.c.count(), 1);
}

#[tokio::test]
async fn patch_never_autodiscovers() {
    let harness = Harness::two_providers("autodiscover-patch", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.set_fallback(model_list(&["gpt-9"]));

    let created = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-auto-patch",
            "base_url": harness.mocks.c.base_url(),
            "models": [{"name": "gpt-5"}]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let id = created.json()["id"].as_str().unwrap().to_string();

    let patched = harness
        .client
        .send(
            ClientRequest::new("PATCH", &format!("/api/provider-keys/{id}"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(serde_json::to_vec(&json!({"models": []})).unwrap()),
        )
        .await;

    assert_eq!(patched.status, 200, "{}", patched.text());
    let body = patched.json();
    assert_eq!(body["models"], json!([]));
    assert!(
        body.get("autodiscover").is_none(),
        "更新响应不得携带 autodiscover：{body}"
    );
    assert_eq!(harness.mocks.c.count(), 0, "PATCH 不得发起上游调用");
}

#[tokio::test]
async fn service_account_credentials_skip_autodiscover() {
    let harness = Harness::two_providers("autodiscover-sa", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.set_fallback(model_list(&["gpt-9"]));
    let service_account = json!({
        "type": "service_account",
        "project_id": "demo",
        "client_email": "runner@demo.iam.gserviceaccount.com",
        "private_key": "-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----\n",
        "token_uri": "https://oauth2.googleapis.com/token"
    });

    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "google-vertex-anthropic-custom",
            "credential_kind": "google_service_account",
            "service_account_json": service_account.to_string(),
            "base_url": harness.mocks.c.base_url(),
            "path_vars": {"project": "demo", "location": "us-east5"}
        }),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(body["credential_kind"], "google_service_account");
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["autodiscover"]["status"], "skipped");
    assert_no_error(&body);
    assert_eq!(harness.mocks.c.count(), 0, "SA 凭据不得发起上游调用");
}

#[tokio::test]
async fn timeout_is_failure_with_bounded_response_time() {
    let harness = Harness::two_providers("autodiscover-timeout", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(MockResponse::delayed_body(
        200,
        r#"{"data":[{"id":"gpt-5"}]}"#,
        Duration::from_secs(10),
    ));

    let started = Instant::now();
    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-auto-timeout",
            "base_url": harness.mocks.c.base_url()
        }),
    )
    .await;
    let elapsed = started.elapsed();

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["autodiscover"]["status"], "failed");
    assert!(
        body["autodiscover"]["error"]
            .as_str()
            .is_some_and(|text| !text.is_empty()),
        "{body}"
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "自动拉取必须有界（实际 {elapsed:?}）"
    );
}
