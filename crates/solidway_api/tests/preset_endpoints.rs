//! 预设端点的模板化、字段锁定与凭据类型约束。

mod common;

use common::{ClientRequest, Harness};
use serde_json::json;

async fn create_route(harness: &Harness, cookie: &str, name: &str) -> String {
    let response = harness
        .client
        .send(
            ClientRequest::post_json("/api/routes", &json!({"name": name}))
                .header("cookie", cookie),
        )
        .await;
    assert_eq!(response.status, 201, "创建路由应成功：{}", response.text());
    response.json()["id"].as_str().unwrap().to_string()
}

async fn create_provider(
    harness: &Harness,
    cookie: &str,
    route_id: &str,
    body: serde_json::Value,
) -> common::ClientResponse {
    harness
        .client
        .send(
            ClientRequest::post_json(&format!("/api/routes/{route_id}/provider-keys"), &body)
                .header("cookie", cookie),
        )
        .await
}

fn service_account(email: &str) -> serde_json::Value {
    json!({
        "type": "service_account",
        "project_id": "demo",
        "client_email": email,
        "private_key": "-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----\n",
        "token_uri": "https://oauth2.googleapis.com/token"
    })
}

#[tokio::test]
async fn preset_fields_are_locked() {
    let harness = Harness::two_providers("preset-locked", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "locked").await;

    for (field, body) in [
        (
            "base_url",
            json!({"provider": "openai", "api_key": "sk-1", "base_url": "https://proxy.example.com/v1"}),
        ),
        (
            "api_formats",
            json!({"provider": "openai", "api_key": "sk-2", "api_formats": ["chat_completions"]}),
        ),
        (
            "auth_scheme",
            json!({"provider": "openai", "api_key": "sk-3", "auth_scheme": "bearer"}),
        ),
    ] {
        let response = create_provider(&harness, &cookie, &route_id, body).await;
        assert_eq!(response.status, 422, "{field}: {}", response.text());
        assert_eq!(
            response.json()["error"]["code"],
            "preset_field_immutable",
            "{field}"
        );
    }

    // 预设条目不带锁定字段时可以创建，端点由模板展开。
    let ok = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({"provider": "openai", "api_key": "sk-ok", "models": [{"name": "gpt-5"}]}),
    )
    .await;
    assert_eq!(ok.status, 201, "{}", ok.text());
    assert_eq!(ok.json()["base_url"], "https://api.openai.com/v1");
    assert_eq!(
        ok.json()["api_formats"],
        json!(["chat_completions", "responses"])
    );
}

#[tokio::test]
async fn template_variables_expand_and_validate() {
    let harness = Harness::two_providers("preset-template", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "template").await;

    let azure = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "azure",
            "api_key": "sk-azure",
            "base_url_vars": {"resource": "my-resource"},
            "path_vars": {"deployment": "gpt-4o-prod", "api_version": "2024-10-21"},
            "models": [{"name": "gpt-4o"}]
        }),
    )
    .await;
    assert_eq!(azure.status, 201, "{}", azure.text());
    assert_eq!(
        azure.json()["base_url"],
        "https://my-resource.openai.azure.com"
    );
    assert_eq!(azure.json()["auth_scheme"], "api-key");
    assert_eq!(
        azure.json()["options"]["base_url_vars"]["resource"],
        "my-resource"
    );
    assert_eq!(
        azure.json()["options"]["path_params"]["deployment"],
        "gpt-4o-prod"
    );
    assert_eq!(
        azure.json()["options"]["path_template"],
        "/openai/deployments/{deployment}/{endpoint}?api-version={api_version}"
    );

    let missing = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({"provider": "azure", "api_key": "sk-azure-2", "models": [{"name": "m"}]}),
    )
    .await;
    assert_eq!(missing.status, 422, "{}", missing.text());
    assert_eq!(missing.json()["error"]["code"], "missing_base_url_var");

    // URL 变量齐全但缺路径参数 → missing_path_var。
    let missing_path = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "azure",
            "api_key": "sk-azure-4",
            "base_url_vars": {"resource": "my-resource"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(missing_path.status, 422, "{}", missing_path.text());
    assert_eq!(missing_path.json()["error"]["code"], "missing_path_var");

    // 路径参数非法 → invalid_path_var。
    let invalid_path = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "azure",
            "api_key": "sk-azure-5",
            "base_url_vars": {"resource": "my-resource"},
            "path_vars": {"deployment": "gpt-4o-prod", "api_version": "2024/10/21"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(invalid_path.status, 422, "{}", invalid_path.text());
    assert_eq!(invalid_path.json()["error"]["code"], "invalid_path_var");

    let invalid = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "azure",
            "api_key": "sk-azure-3",
            "base_url_vars": {"resource": "bad/name"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(invalid.status, 422, "{}", invalid.text());
    assert_eq!(invalid.json()["error"]["code"], "invalid_base_url_var");

    // host-only 变量拒绝带路径。
    let host_only = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "databricks",
            "api_key": "sk-db",
            "base_url_vars": {"DATABRICKS_HOST": "dbc-1.cloud.databricks.com/ai-gateway"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(host_only.status, 422, "{}", host_only.text());
    assert_eq!(host_only.json()["error"]["code"], "invalid_base_url_var");
}

#[tokio::test]
async fn deploy_specific_preset_uses_full_url_variable() {
    let harness = Harness::two_providers("preset-full-url", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "full-url").await;

    let created = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "openai-compatible",
            "api_key": "sk-compat",
            "base_url_vars": {"BASE_URL": "https://proxy.example.com/v1/"},
            "models": [{"name": "gpt-5"}]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());
    assert_eq!(created.json()["base_url"], "https://proxy.example.com/v1");
}

#[tokio::test]
async fn vertex_global_preset_uses_templated_path() {
    let harness = Harness::two_providers("preset-vertex-global", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "vertex-global").await;

    let created = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "google-vertex-global-anthropic",
            "service_account_json": service_account("global@demo.iam.gserviceaccount.com").to_string(),
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let body = created.json();
    assert_eq!(body["base_url"], "https://aiplatform.googleapis.com");
    assert_eq!(body["options"]["path_params"]["project"], "demo");
    assert!(
        body["options"]["path_template"]
            .as_str()
            .unwrap_or_default()
            .contains("/locations/global/"),
        "{}",
        body["options"]["path_template"]
    );
    assert_eq!(body["api_formats"], json!(["anthropic_messages"]));
    // Service Account 不可探测，不得呈现为健康。
    assert_eq!(body["probeable"], false);

    // 目录侧：global 条目无变量，regional 条目有 location。
    let presets = harness
        .client
        .send(ClientRequest::new("GET", "/api/providers").header("cookie", &cookie))
        .await
        .json();
    let list = presets.as_array().expect("array");
    let global = list
        .iter()
        .find(|preset| preset["key"] == "google-vertex-global-anthropic")
        .expect("global preset");
    assert_eq!(global["base_url_variables"], json!([]));
    assert_eq!(
        global["base_url_template"],
        "https://aiplatform.googleapis.com"
    );
    assert_eq!(global["path_variables"][0]["name"], "project");
    let regional = list
        .iter()
        .find(|preset| preset["key"] == "google-vertex-anthropic")
        .expect("regional preset");
    assert_eq!(regional["base_url_variables"][0]["name"], "location");
    // 区域形态的路径参数是 project + location（location 与 URL 变量同名，UI 只填一次）。
    assert_eq!(regional["path_variables"].as_array().map(Vec::len), Some(2));
    assert_eq!(regional["path_variables"][1]["name"], "location");
    assert_eq!(regional["group"], "Vertex AI");
}

#[tokio::test]
async fn credential_kind_is_detected_from_content() {
    let harness = Harness::two_providers("preset-credential", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "credential").await;

    // 未提供 credential_kind 也能识别 Service Account。
    let detected = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "google-vertex-anthropic",
            "service_account_json": service_account("sa@demo.iam.gserviceaccount.com").to_string(),
            "base_url_vars": {"location": "us-east5"},
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(detected.status, 201, "{}", detected.text());
    assert_eq!(detected.json()["credential_kind"], "google_service_account");

    // 提示与内容冲突 → 422。
    let mismatch = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "google-vertex-anthropic",
            "credential_kind": "api_key",
            "service_account_json": service_account("sa2@demo.iam.gserviceaccount.com").to_string(),
            "base_url_vars": {"location": "us-east5"},
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(mismatch.status, 422, "{}", mismatch.text());
    assert_eq!(mismatch.json()["error"]["code"], "credential_kind_mismatch");
}

#[tokio::test]
async fn service_account_requires_google_endpoint() {
    let harness = Harness::two_providers("preset-sa-scope", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "sa-scope").await;

    let rejected = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "my-service",
            "service_account_json": service_account("sa@demo.iam.gserviceaccount.com").to_string(),
            "base_url": "https://api.example.com/v1",
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(rejected.status, 422, "{}", rejected.text());
    assert_eq!(
        rejected.json()["error"]["code"],
        "service_account_requires_google_preset"
    );

    // 自定义端点命中 `{prefix}.googleapis.com` 模式时放行。
    let allowed = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "vertex-gateway",
            "service_account_json": service_account("sa@demo.iam.gserviceaccount.com").to_string(),
            "base_url": "https://gw.googleapis.com",
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(allowed.status, 201, "{}", allowed.text());
    assert_eq!(allowed.json()["auth_scheme"], "google_service_account");
}

#[tokio::test]
async fn custom_copy_inherits_adapter_and_allows_bearer() {
    let harness = Harness::two_providers("preset-custom-copy", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "custom-copy").await;

    // 复制为自定义后字段自由：显式 base_url / 格式 / 鉴权均被接受。
    let copied = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "openai-custom",
            "api_key": "sk-copy",
            "base_url": "https://proxy.example.com/v1",
            "api_formats": ["responses"],
            "auth_scheme": "x-api-key",
            "models": [{"name": "gpt-5"}]
        }),
    )
    .await;
    assert_eq!(copied.status, 201, "{}", copied.text());
    assert_eq!(copied.json()["api_formats"], json!(["responses"]));
    assert_eq!(copied.json()["auth_scheme"], "x-api-key");

    // Vertex 副本仍继承 Vertex 适配器（反向依赖 `-custom` 命名约定）。
    let vertex_copy = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "google-vertex-anthropic-custom",
            "api_key": "sk-vertex",
            "auth_scheme": "bearer",
            "base_url": "https://vertex-gw.example.com",
            "api_formats": ["anthropic_messages"],
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(vertex_copy.status, 201, "{}", vertex_copy.text());
}

#[tokio::test]
async fn discover_rejects_service_account_without_upstream_call() {
    let harness = Harness::two_providers("preset-discover-sa", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "discover-sa").await;

    let created = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "google-vertex-anthropic",
            "service_account_json": service_account("sa@demo.iam.gserviceaccount.com").to_string(),
            "base_url_vars": {"location": "us-east5"},
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let provider_id = created.json()["id"].as_str().unwrap().to_string();

    let response = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/models/discover",
                &json!({
                    "provider": "google-vertex-anthropic",
                    "base_url": "https://us-east5-aiplatform.googleapis.com",
                    "api_key": "",
                    "provider_key_id": provider_id
                }),
            )
            .header("cookie", &cookie),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["models"], json!([]));
    assert_eq!(body["error_code"], "unsupported_credential_kind");
}

#[tokio::test]
async fn probeable_is_false_for_unprobeable_credentials() {
    let harness = Harness::two_providers("preset-probeable", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "probeable").await;

    let normal = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({"provider": "openai", "api_key": "sk-p1", "models": [{"name": "gpt-5"}]}),
    )
    .await;
    assert_eq!(normal.status, 201, "{}", normal.text());
    assert_eq!(normal.json()["probeable"], true);

    let sa = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "google-vertex-anthropic",
            "service_account_json": service_account("sa@demo.iam.gserviceaccount.com").to_string(),
            "base_url_vars": {"location": "us-east5"},
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(sa.status, 201, "{}", sa.text());
    assert_eq!(sa.json()["probeable"], false);

    let empty_models = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "probeable-proxy",
            "api_key": "sk-p2",
            "base_url": "https://proxy.example.com/v1",
            "models": []
        }),
    )
    .await;
    assert_eq!(empty_models.status, 201, "{}", empty_models.text());
    assert_eq!(empty_models.json()["probeable"], false);
}

fn aws_credentials() -> serde_json::Value {
    json!({
        "access_key_id": "AKIAIOSFODNN7EXAMPLE",
        "secret_access_key": "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY"
    })
}

#[tokio::test]
async fn bedrock_anthropic_accepts_aws_credentials() {
    let harness = Harness::two_providers("bedrock-anthropic", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "bedrock").await;

    let created = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "bedrock-anthropic",
            "api_key": aws_credentials().to_string(),
            "base_url_vars": {"region": "us-east-1"},
            "models": [{"name": "anthropic.claude-3-5-sonnet-20240620-v1:0"}]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let body = created.json();
    assert_eq!(body["credential_kind"], "aws_sigv4");
    assert_eq!(body["auth_scheme"], "sigv4");
    assert_eq!(body["credential_masked"], "aws:AKIA****MPLE");
    assert_eq!(
        body["base_url"],
        "https://bedrock-runtime.us-east-1.amazonaws.com"
    );
    assert_eq!(body["api_formats"], json!(["anthropic_messages"]));
    assert_eq!(body["probeable"], false);
    assert_eq!(body["options"]["aws_service"], "bedrock");
    assert_eq!(body["options"]["anthropic_version"], "bedrock-2023-05-31");
    assert_eq!(body["options"]["path_actions"]["unary"], "invoke");
    assert_eq!(
        body["options"]["path_actions"]["stream"],
        "invoke-with-response-stream"
    );
    assert_eq!(body["options"]["base_url_vars"]["region"], "us-east-1");
    // AWS 凭据不触发模型自动拉取。
    assert_eq!(body["autodiscover"]["status"], "skipped");
}

#[tokio::test]
async fn aws_credentials_require_sigv4_and_valid_fields() {
    let harness = Harness::two_providers("bedrock-errors", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "bedrock-errors").await;

    // 缺 secret → 内容判定不算 AWS 凭据，按 api_key 校验失败。
    let missing = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "bedrock-anthropic",
            "api_key": json!({"access_key_id": "AKIA"}).to_string(),
            "base_url_vars": {"region": "us-east-1"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(missing.status, 422, "{}", missing.text());

    // 显式声明类型但内容不符 → 冲突。
    let mismatch = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "bedrock-anthropic",
            "credential_kind": "api_key",
            "api_key": aws_credentials().to_string(),
            "base_url_vars": {"region": "us-east-1"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(mismatch.status, 422, "{}", mismatch.text());
    assert_eq!(mismatch.json()["error"]["code"], "credential_kind_mismatch");

    // 自定义条目用 AWS 凭据但声明 bearer → 鉴权方式不兼容。
    let incompatible = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "aws-proxy",
            "api_key": aws_credentials().to_string(),
            "base_url": "https://bedrock-proxy.example.com",
            "auth_scheme": "bearer",
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(incompatible.status, 422, "{}", incompatible.text());
    assert_eq!(
        incompatible.json()["error"]["code"],
        "incompatible_auth_scheme"
    );
}

#[tokio::test]
async fn discover_rejects_aws_credentials() {
    let harness = Harness::two_providers("bedrock-discover", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "bedrock-discover").await;

    let created = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "bedrock-anthropic",
            "api_key": aws_credentials().to_string(),
            "base_url_vars": {"region": "us-east-1"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let provider_id = created.json()["id"].as_str().unwrap().to_string();

    let response = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/models/discover",
                &json!({
                    "provider": "bedrock-anthropic",
                    "base_url": "https://bedrock-runtime.us-east-1.amazonaws.com",
                    "api_key": "",
                    "provider_key_id": provider_id
                }),
            )
            .header("cookie", &cookie),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["error_code"], "unsupported_credential_kind");
}

#[tokio::test]
async fn bedrock_preset_is_listed_with_actions() {
    let harness = Harness::two_providers("bedrock-preset", "").await;
    let cookie = harness.admin_cookie().await;
    let presets = harness
        .client
        .send(ClientRequest::new("GET", "/api/providers").header("cookie", &cookie))
        .await
        .json();
    let list = presets.as_array().expect("array");
    assert!(
        list.iter().all(|preset| preset["key"] != "amazon-bedrock"),
        "旧 key 应已移除"
    );
    let bedrock = list
        .iter()
        .find(|preset| preset["key"] == "bedrock-anthropic")
        .expect("bedrock-anthropic preset");
    assert_eq!(bedrock["auth_scheme"], "sigv4");
    assert_eq!(bedrock["path_actions"]["unary"], "invoke");
    assert_eq!(bedrock["aws_service"], "bedrock");
    assert_eq!(bedrock["anthropic_version"], "bedrock-2023-05-31");
    assert_eq!(bedrock["api_formats"], json!(["anthropic_messages"]));
}

#[tokio::test]
async fn bedrock_invoke_is_signed_and_rendered() {
    let harness = Harness::two_providers("bedrock-forward", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.c.set_fallback(common::MockResponse::new(
        200,
        r#"{"id":"msg","type":"message","model":"claude"}"#,
    ));

    // 用 `-custom` 副本把 base_url 指向 mock；路径模板/动作名/anthropic_version 继承预设。
    let created = create_provider(
        &harness,
        &cookie,
        "r-main",
        json!({
            "provider": "bedrock-anthropic-custom",
            "api_key": aws_credentials().to_string(),
            // 模板是绝对路径（/model/...），base_url 用主机根。
            "base_url": harness.mocks.c.base_url().trim_end_matches("/v1"),
            "options": {"region": "us-east-1"},
            "auth_scheme": "sigv4",
            "models": [{
                "name": "claude",
                "upstream": "anthropic.claude-3-5-sonnet-20240620-v1:0"
            }]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());

    let response = harness
        .client
        .send(
            ClientRequest::post_json(
                "/v1/messages",
                &json!({"model": "claude", "max_tokens": 8, "messages": [{"role": "user", "content": "hi"}]}),
            )
            .header("x-api-key", common::TENANT_KEY),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());

    let request = harness.mocks.c.last_request().expect("upstream request");
    assert_eq!(
        request.path,
        "/model/anthropic.claude-3-5-sonnet-20240620-v1:0:invoke"
    );
    let authorization = request.header("authorization").expect("authorization");
    assert!(
        authorization.starts_with("AWS4-HMAC-SHA256 Credential="),
        "{authorization}"
    );
    assert!(
        authorization.contains("/us-east-1/bedrock/aws4_request"),
        "{authorization}"
    );
    assert!(request.header("x-amz-date").is_some());
    let body = request.json();
    assert!(body.get("model").is_none(), "{body}");
    assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");
}
