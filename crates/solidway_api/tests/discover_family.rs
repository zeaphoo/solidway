//! 家族感知的模型清单获取：地址构造、list 形状解析与失败二分。

mod common;

use common::{ClientRequest, ClientResponse, Harness, MockResponse};
use serde_json::{Value, json};

fn no_version_base(harness: &Harness) -> String {
    // mock 的 base_url 固定以 `/v1` 结尾；去掉后用于验证「按家族补版本段」。
    harness
        .mocks
        .c
        .base_url()
        .trim_end_matches("/v1")
        .to_string()
}

async fn discover(harness: &Harness, cookie: &str, body: Value) -> ClientResponse {
    harness
        .client
        .send(ClientRequest::post_json("/api/models/discover", &body).header("cookie", cookie))
        .await
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

fn json_list() -> MockResponse {
    MockResponse::new(
        200,
        r#"{"data":[{"id":"claude-x","display_name":"Claude X"}]}"#,
    )
    .with_header("content-type", "application/json")
}

fn last_path(harness: &Harness) -> String {
    harness
        .mocks
        .c
        .last_request()
        .expect("upstream request")
        .path
}

#[tokio::test]
async fn anthropic_adds_v1_once_and_parses_list_shape() {
    let harness = Harness::two_providers("family-anthropic", "").await;
    let cookie = harness.admin_cookie().await;
    let root = no_version_base(&harness);

    harness.mocks.c.script_one(json_list());
    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "anthropic-custom",
            "base_url": format!("{root}/apps/anthropic"),
            "api_key": "sk-ant",
            "auth_scheme": "x-api-key",
            "api_formats": ["anthropic_messages"]
        }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!(["claude-x"]));
    assert!(body.get("error").is_none(), "{body}");
    assert!(body.get("error_code").is_none(), "{body}");
    assert_eq!(last_path(&harness), "/apps/anthropic/v1/models");
    let request = harness.mocks.c.last_request().expect("request");
    assert_eq!(request.header("x-api-key"), Some("sk-ant"));

    // 已含版本段时不重复补。
    harness.mocks.c.script_one(json_list());
    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "anthropic-custom",
            "base_url": format!("{root}/apps/anthropic/v1"),
            "api_key": "sk-ant",
            "auth_scheme": "x-api-key",
            "api_formats": ["anthropic_messages"]
        }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(last_path(&harness), "/apps/anthropic/v1/models");
}

#[tokio::test]
async fn omitted_formats_are_inferred_from_provider_preset() {
    let harness = Harness::two_providers("family-preset", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(
        MockResponse::new(200, r#"{"data":[{"id":"claude-x"}]}"#)
            .with_header("content-type", "application/json"),
    );

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "anthropic-custom",
            "base_url": no_version_base(&harness),
            "api_key": "sk-ant",
            "auth_scheme": "x-api-key"
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["models"], json!(["claude-x"]));
    assert_eq!(last_path(&harness), "/v1/models");
}

#[tokio::test]
async fn anthropic_404_is_unsupported_with_actionable_message() {
    let harness = Harness::two_providers("family-404", "").await;
    let cookie = harness.admin_cookie().await;
    harness
        .mocks
        .c
        .script_one(MockResponse::new(404, "not found"));

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "anthropic-custom",
            "base_url": no_version_base(&harness),
            "api_key": "sk-ant",
            "auth_scheme": "x-api-key",
            "api_formats": ["anthropic_messages"]
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["error_code"], "unsupported_model_list");
    assert_eq!(body["error"], "上游不支持模型清单拉取，请手动添加模型");
}

#[tokio::test]
async fn shape_mismatch_is_unsupported() {
    let harness = Harness::two_providers("family-shape", "").await;
    let cookie = harness.admin_cookie().await;
    harness
        .mocks
        .c
        .script_one(MockResponse::new(200, "<html>not json</html>"));

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "base_url": no_version_base(&harness),
            "api_key": "sk-shape",
            "api_formats": ["chat_completions"]
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["error_code"], "unsupported_model_list");
}

#[tokio::test]
async fn auth_error_is_not_labelled_unsupported() {
    let harness = Harness::two_providers("family-401", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(MockResponse::new(401, "{}"));

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "anthropic-custom",
            "base_url": no_version_base(&harness),
            "api_key": "sk-bad",
            "auth_scheme": "x-api-key",
            "api_formats": ["anthropic_messages"]
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert!(
        body.get("error_code").is_none(),
        "鉴权错误不得标不支持：{body}"
    );
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|text| text.contains("401")),
        "{body}"
    );
}

#[tokio::test]
async fn openai_branch_never_adds_version_segment() {
    let harness = Harness::two_providers("family-openai", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(
        MockResponse::new(200, r#"{"data":[{"id":"gpt-5"}]}"#)
            .with_header("content-type", "application/json"),
    );

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "openai-custom",
            "base_url": no_version_base(&harness),
            "api_key": "sk-openai",
            "api_formats": ["chat_completions"]
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["models"], json!(["gpt-5"]));
    assert_eq!(last_path(&harness), "/models");
}

#[tokio::test]
async fn mixed_formats_prefer_openai_branch() {
    let harness = Harness::two_providers("family-mixed", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.script_one(
        MockResponse::new(200, r#"{"data":[{"id":"m"}]}"#)
            .with_header("content-type", "application/json"),
    );

    let response = discover(
        &harness,
        &cookie,
        json!({
            "provider": "my-proxy",
            "base_url": format!("{}/proxy/x", no_version_base(&harness)),
            "api_key": "sk-mixed",
            "api_formats": ["anthropic_messages", "chat_completions"]
        }),
    )
    .await;

    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["models"], json!(["m"]));
    assert_eq!(last_path(&harness), "/proxy/x/models");
}

#[tokio::test]
async fn autodiscover_passes_error_code_through() {
    let harness = Harness::two_providers("family-autodiscover-404", "").await;
    let cookie = harness.admin_cookie().await;
    harness
        .mocks
        .c
        .script_one(MockResponse::new(404, "not found"));

    let response = create_provider(
        &harness,
        &cookie,
        json!({
            "provider": "anthropic-custom",
            "api_key": "sk-ant",
            "base_url": no_version_base(&harness),
            "api_formats": ["anthropic_messages"]
        }),
    )
    .await;

    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["autodiscover"]["status"], "failed");
    assert_eq!(body["autodiscover"]["error_code"], "unsupported_model_list");
    assert_eq!(last_path(&harness), "/v1/models");
}
