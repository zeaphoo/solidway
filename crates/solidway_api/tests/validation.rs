//! 写操作校验与配置即时生效（develop `tests/config.rs` 的行为等价物）。
//!
//! 背景：本项目**移除了 TOML 配置路径**（配置来自 DB，由管理 API 写入），
//! 因此 develop 的「启动期解析/校验/CLI/环境变量/重启生效」类用例在本项目
//! **结构性不适用**；其行为语义改由本文件在 HTTP 层覆盖：
//!   - 非法 `api_key` / `base_url` → 422；
//!   - `models` 可为空数组（= 不服务任何模型），重复项去重；
//!   - 配置变更**即时生效**（无需重启；这与 develop 的「重启才生效」相反，是有意变更）。

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use common::{ClientRequest, Harness, MockResponse};
use serde_json::json;

/// 建一个路由并返回其 id。
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

#[tokio::test]
async fn illegal_api_key_is_rejected_at_write_time() {
    let harness = Harness::two_providers("validate-api-key", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "v").await;

    // 含非 ASCII / 控制字符的 api_key 无法作为 header 值转发 → 422。
    for bad in ["sk-中文", "sk-\n"] {
        let response = create_provider(
            &harness,
            &cookie,
            &route_id,
            json!({
                "provider": "api-key-proxy",
                "api_key": bad,
                "base_url": "https://proxy.example.com/v1",
                "models": [{"name": "gpt-5"}]
            }),
        )
        .await;
        assert_eq!(
            response.status,
            422,
            "api_key={bad:?} 应被拒：{}",
            response.text()
        );
        assert_eq!(response.json()["error"]["code"], "invalid_request");
    }

    // 合法：可打印 ASCII + 空格。
    let ok = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "api-key-proxy",
            "api_key": "sk-abc_DEF 123",
            "base_url": "https://proxy.example.com/v1",
            "models": [{"name": "gpt-5"}]
        }),
    )
    .await;
    assert_eq!(ok.status, 201, "{}", ok.text());
}

#[tokio::test]
async fn service_account_credential_is_stored_and_masked() {
    let harness = Harness::two_providers("validate-service-account", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "v").await;
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
        &route_id,
        json!({
            "provider": "google-vertex-anthropic",
            "credential_kind": "google_service_account",
            "service_account_json": service_account.to_string(),
            "base_url_vars": {"location": "us-east5"},
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(body["credential_kind"], "google_service_account");
    assert_eq!(
        body["credential_masked"],
        "service_account:runner@demo.iam.gserviceaccount.com"
    );
    assert_eq!(body["auth_scheme"], "google_service_account");
    assert!(body.get("credential_ciphertext").is_none());
    assert!(body.get("api_key").is_none());

    let missing = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "google-vertex-anthropic",
            "credential_kind": "google_service_account",
            "service_account_json": "{\"type\":\"service_account\"}",
            "base_url_vars": {"location": "us-east5"},
            "path_vars": {"project": "demo", "location": "us-east5"},
            "models": [{"name": "claude-sonnet-4"}]
        }),
    )
    .await;
    assert_eq!(missing.status, 422, "{}", missing.text());
}

#[tokio::test]
async fn base_url_accepts_any_http_prefix() {
    let harness = Harness::two_providers("validate-base-url", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "v").await;

    // 根地址、兼容前缀、/v1 都可以（只要求绝对 http(s) URL）。
    for (index, url) in [
        "https://api.deepseek.com",
        "https://api.deepinfra.com/v1/openai",
        "https://api.openai.com/v1",
    ]
    .iter()
    .enumerate()
    {
        let ok = create_provider(
            &harness,
            &cookie,
            &route_id,
            json!({
                "provider": "base-url-proxy",
                "api_key": format!("sk-ok-{index}"),
                "base_url": url,
                "models": [{"name": "gpt-5"}]
            }),
        )
        .await;
        assert_eq!(ok.status, 201, "{url}: {}", ok.text());
    }

    for (index, url) in [
        "api.deepseek.com",
        "ftp://api.deepseek.com",
        "https:///v1",
        "https://api.deepseek.com?x=1",
        "https://api.deepseek.com#x",
    ]
    .iter()
    .enumerate()
    {
        let bad = create_provider(
            &harness,
            &cookie,
            &route_id,
            json!({
                "provider": "base-url-proxy",
                "api_key": format!("sk-bad-{index}"),
                "base_url": url,
                "models": []
            }),
        )
        .await;
        assert_eq!(bad.status, 422, "{url}: {}", bad.text());
    }
}

#[tokio::test]
async fn models_are_optional_and_deduplicated() {
    let harness = Harness::two_providers("validate-models", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "v").await;

    // 空数组允许（= 不服务任何模型）。创建时会自动拉取，这里让上游失败以保持空清单。
    harness.mocks.c.script_one(MockResponse::new(500, "{}"));
    let empty = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "models-proxy",
            "api_key": "sk-empty",
            "base_url": harness.mocks.c.base_url(),
            "models": []
        }),
    )
    .await;
    assert_eq!(empty.status, 201, "{}", empty.text());
    assert_eq!(empty.json()["models"], json!([]));
    assert_eq!(empty.json()["autodiscover"]["status"], "failed");

    // 重复与空白项被去重 / 丢弃。
    let dup = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "models-proxy",
            "api_key": "sk-dup",
            "base_url": "https://proxy.example.com/v1",
            "models": [{"name": "gpt-5"}, {"name": " gpt-5 "}, {"name": ""}, {"name": "gpt-5-mini", "upstream": "gpt-5-mini-2025"}]
        }),
    )
    .await;
    assert_eq!(dup.status, 201, "{}", dup.text());
    assert_eq!(
        dup.json()["models"],
        json!([
            {"name": "gpt-5", "upstream": "gpt-5"},
            {"name": "gpt-5-mini", "upstream": "gpt-5-mini-2025"}
        ])
    );
}

#[tokio::test]
async fn missing_version_segment_is_saved_without_warning_or_rewrite() {
    // 归一化改由转发目标构造负责：保存不再告警，也不改写用户输入的 base_url。
    let harness = Harness::two_providers("validate-version", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "v").await;

    let created = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "anthropic-custom",
            "api_key": "sk-anthropic",
            "base_url": "https://api.example.com/apps/anthropic",
            "api_formats": ["anthropic_messages"],
            "models": [{"name": "claude-sonnet-4-5"}]
        }),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let body = created.json();
    assert!(body.get("warning").is_none(), "不再返回版本段告警：{body}");
    assert_eq!(
        body["base_url"],
        json!("https://api.example.com/apps/anthropic"),
        "保存 MUST NOT 改写 base_url"
    );

    let fetched = harness
        .client
        .send(
            ClientRequest::new(
                "GET",
                &format!("/api/provider-keys/{}", body["id"].as_str().unwrap()),
            )
            .header("cookie", &cookie),
        )
        .await;
    assert_eq!(fetched.status, 200, "{}", fetched.text());
    assert_eq!(
        fetched.json()["base_url"],
        json!("https://api.example.com/apps/anthropic")
    );
}

#[tokio::test]
async fn config_changes_take_effect_immediately() {
    // develop 的口径是「改配置需重启」；本项目配置来自 DB，**即时生效**（有意变更）。
    let harness = Harness::two_providers("live-config", "").await;
    let cookie = harness.admin_cookie().await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    // 新增一个只服务 gpt-9 的 provider → 不重启即可路由 gpt-9。
    let response = create_provider(
        &harness,
        &cookie,
        "r-main",
        json!({
            "provider": "live-proxy",
            "api_key": "sk-live",
            "base_url": harness.mocks.c.base_url(),
            "models": [{"name": "gpt-9"}]
        }),
    )
    .await;
    assert_eq!(response.status, 201, "{}", response.text());

    let models = harness
        .client
        .send(ClientRequest::new("GET", "/v1/models").bearer(common::TENANT_KEY))
        .await;
    let ids: Vec<String> = models.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        ids.contains(&"gpt-9".to_string()),
        "无需重启即可见新模型：{ids:?}"
    );

    // 停用该 provider → 立即从模型并集消失。
    let provider_id = response.json()["id"].as_str().unwrap().to_string();
    let patch = harness
        .client
        .send(
            ClientRequest::new("PATCH", &format!("/api/provider-keys/{provider_id}"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(br#"{"status":"inactive"}"#.to_vec()),
        )
        .await;
    assert_eq!(patch.status, 200, "{}", patch.text());

    let models = harness
        .client
        .send(ClientRequest::new("GET", "/v1/models").bearer(common::TENANT_KEY))
        .await;
    let ids: Vec<String> = models.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert!(
        !ids.contains(&"gpt-9".to_string()),
        "停用后立即消失：{ids:?}"
    );
}

#[tokio::test]
async fn non_admin_cannot_access_system_endpoints() {
    let harness = Harness::two_providers("admin-only-system", "").await;
    let admin = harness.admin_cookie().await;

    let created = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/users",
                &json!({"username": "member", "password": "member-pass-123", "role": "user"}),
            )
            .header("cookie", &admin),
        )
        .await;
    assert_eq!(
        created.status,
        201,
        "创建普通用户应成功：{}",
        created.text()
    );

    let login = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({"username": "member", "password": "member-pass-123"}),
        ))
        .await;
    assert_eq!(login.status, 200, "普通用户登录应成功：{}", login.text());
    let cookie = login
        .header("set-cookie")
        .expect("应有 set-cookie")
        .split(';')
        .next()
        .expect("cookie 段")
        .to_string();

    // 系统页与用户管理相关端点都只允许 admin。
    for path in [
        "/api/system",
        "/api/app/info",
        "/api/settings",
        "/api/users",
    ] {
        let response = harness
            .client
            .send(ClientRequest::new("GET", path).header("cookie", &cookie))
            .await;
        assert_eq!(
            response.status,
            403,
            "{path} 应仅 admin 可访问：{}",
            response.text()
        );
        assert_eq!(response.json()["error"]["code"], "forbidden", "{path}");
    }

    // admin 仍可正常访问。
    let ok = harness
        .client
        .send(ClientRequest::new("GET", "/api/system").header("cookie", &admin))
        .await;
    assert_eq!(ok.status, 200, "{}", ok.text());
}

#[tokio::test]
async fn api_formats_default_from_preset_and_validate() {
    let harness = Harness::two_providers("validate-api-formats", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "v").await;

    // openai 预设默认支持 chat completions + responses。
    let openai = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({"provider": "openai", "api_key": "sk-fmt-1", "models": [{"name": "gpt-5"}]}),
    )
    .await;
    assert_eq!(openai.status, 201, "{}", openai.text());
    assert_eq!(
        openai.json()["api_formats"],
        json!(["chat_completions", "responses"])
    );

    // OpenAI 兼容上游默认只有 chat completions。
    let compat = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "openai-compatible",
            "api_key": "sk-fmt-2",
            "base_url_vars": {"BASE_URL": "https://compat.example.com/v1"},
            "models": [{"name": "m"}]
        }),
    )
    .await;
    assert_eq!(compat.status, 201, "{}", compat.text());
    assert_eq!(compat.json()["api_formats"], json!(["chat_completions"]));

    // 显式声明：去重 + 去空白。
    let explicit = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "compat-proxy",
            "api_key": "sk-fmt-3",
            "base_url": "https://compat.example.com/v1",
            "models": [{"name": "m"}],
            "api_formats": ["responses", " chat_completions ", "responses"]
        }),
    )
    .await;
    assert_eq!(explicit.status, 201, "{}", explicit.text());
    assert_eq!(
        explicit.json()["api_formats"],
        json!(["responses", "chat_completions"])
    );

    // 未知取值 → 422，不落库。
    let bad = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "fmt-proxy",
            "api_key": "sk-fmt-4",
            "base_url": "https://proxy.example.com/v1",
            "models": [{"name": "gpt-5"}],
            "api_formats": ["openai"]
        }),
    )
    .await;
    assert_eq!(bad.status, 422, "{}", bad.text());
    assert_eq!(bad.json()["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn provider_presets_expose_default_api_formats() {
    let harness = Harness::two_providers("preset-api-formats", "").await;
    let cookie = harness.admin_cookie().await;
    let response = harness
        .client
        .send(ClientRequest::new("GET", "/api/providers").header("cookie", &cookie))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let presets = response.json();
    let openai = presets
        .as_array()
        .expect("array")
        .iter()
        .find(|preset| preset["key"] == "openai")
        .expect("openai preset");
    assert_eq!(
        openai["api_formats"],
        json!(["chat_completions", "responses"])
    );

    let google_openai = presets
        .as_array()
        .expect("array")
        .iter()
        .find(|preset| preset["key"] == "google-openai")
        .expect("google-openai preset");
    assert_eq!(
        google_openai["base_url_template"],
        "https://generativelanguage.googleapis.com/v1beta/openai"
    );
    assert_eq!(
        openai["locked_fields"],
        json!(["base_url", "api_formats", "auth_scheme", "provider"])
    );
    assert_eq!(google_openai["auth_scheme"], "bearer");
    assert_eq!(google_openai["api_formats"], json!(["chat_completions"]));
    assert_eq!(openai["kind"], "api");
    assert_eq!(openai["name_zh"], "OpenAI");
    assert_eq!(google_openai["kind"], "api");

    let alibaba_coding = presets
        .as_array()
        .expect("array")
        .iter()
        .find(|preset| preset["key"] == "alibaba-coding-plan")
        .expect("alibaba coding plan");
    assert_eq!(alibaba_coding["name"], "Alibaba Coding Plan");
    assert_eq!(alibaba_coding["name_zh"], "阿里云编程套餐");

    let minimax = presets
        .as_array()
        .expect("array")
        .iter()
        .find(|preset| preset["key"] == "minimax")
        .expect("minimax preset");
    assert_eq!(minimax["kind"], "api");
    assert_eq!(minimax["api_formats"], json!(["anthropic_messages"]));
    assert!(minimax["note"].as_str().is_some());

    let coding_plan = presets
        .as_array()
        .expect("array")
        .iter()
        .find(|preset| preset["key"] == "zai-coding-plan")
        .expect("zai coding plan");
    assert_eq!(coding_plan["kind"], "coding_plan");

    for key in [
        "zhipuai",
        "volcengine",
        "tencent-tokenhub",
        "xiaomi",
        "minimax-cn",
        "siliconflow-cn",
        "stepfun",
        "sensenova",
        "qianfan",
        "xfyun",
        "hunyuan",
        "baichuan",
        "lingyiwanwu",
    ] {
        let preset = presets
            .as_array()
            .expect("array")
            .iter()
            .find(|preset| preset["key"] == key)
            .unwrap_or_else(|| panic!("缺少中国服务商预设 {key}"));
        assert_eq!(preset["featured"], true, "{key} 应在常用分组");
    }

    for preset in presets.as_array().expect("array") {
        if matches!(preset["kind"].as_str(), Some("coding_plan" | "token_plan")) {
            assert_eq!(
                preset["featured"], true,
                "{} 编程/Token 套餐应在常用分组",
                preset["key"]
            );
        }
    }

    for key in ["gitlab", "amazon-bedrock"] {
        assert!(
            !presets
                .as_array()
                .expect("array")
                .iter()
                .any(|preset| preset["key"] == key),
            "不兼容预设 {key} 应已移除"
        );
    }

    for key in ["google", "google-vertex", "google-vertex-global"] {
        assert!(
            !presets
                .as_array()
                .expect("array")
                .iter()
                .any(|preset| preset["key"] == key),
            "Gemini 原生预设 {key} 应已移除"
        );
    }
}

#[tokio::test]
async fn custom_provider_is_accepted_with_defaults() {
    let harness = Harness::two_providers("validate-custom-provider", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "v").await;

    let response = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "my-proxy",
            "api_key": "sk-custom",
            "base_url": "https://proxy.example.com/v1",
            "models": [{"name": "gpt-5"}]
        }),
    )
    .await;
    assert_eq!(response.status, 201, "{}", response.text());
    let body = response.json();
    assert_eq!(body["provider"], "my-proxy");
    assert_eq!(body["api_formats"], json!(["chat_completions"]));
    assert_eq!(body["auth_scheme"], "bearer");

    // 显式指定鉴权方式（自定义上游可用 x-api-key 等）。
    let explicit = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "my-header-proxy",
            "api_key": "sk-custom-2",
            "base_url": "https://proxy2.example.com/v1",
            "auth_scheme": "x-api-key",
            "models": [{"name": "gpt-5"}]
        }),
    )
    .await;
    assert_eq!(explicit.status, 201, "{}", explicit.text());
    assert_eq!(explicit.json()["auth_scheme"], "x-api-key");

    // 未知鉴权方式 → 422。
    let bad = create_provider(
        &harness,
        &cookie,
        &route_id,
        json!({
            "provider": "my-proxy",
            "api_key": "sk-custom-3",
            "base_url": "https://proxy.example.com/v1",
            "auth_scheme": "oauth",
            "models": []
        }),
    )
    .await;
    assert_eq!(bad.status, 422, "{}", bad.text());
    assert_eq!(bad.json()["error"]["code"], "invalid_request");
}

#[tokio::test]
async fn new_route_ids_are_nine_digit_numbers() {
    let harness = Harness::two_providers("route-id-format", "").await;
    let cookie = harness.admin_cookie().await;
    for index in 0..3 {
        let id = create_route(&harness, &cookie, &format!("route-{index}")).await;
        assert_eq!(id.len(), 9, "路由 ID 应为 9 位数字：{id}");
        assert!(
            id.chars().all(|ch| ch.is_ascii_digit()),
            "路由 ID 只能包含数字：{id}"
        );
        assert!(!id.starts_with('0'), "路由 ID 首位不能为 0：{id}");
    }
}

#[tokio::test]
async fn access_urls_are_admin_managed_and_user_readable() {
    let harness = Harness::two_providers("access-urls", "").await;
    let admin = harness.admin_cookie().await;

    let initial = harness
        .client
        .send(ClientRequest::new("GET", "/api/access").header("cookie", &admin))
        .await;
    assert_eq!(initial.status, 200, "{}", initial.text());
    assert_eq!(initial.json()["public_origins"], json!([]));

    let patch = harness
        .client
        .send(
            ClientRequest::new("PATCH", "/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &admin)
                .raw_body(
                    br#"{"access":{"public_origins":["https://gw.example.com/"," http://192.168.1.10:6565 "]}}"#
                        .to_vec(),
                ),
        )
        .await;
    assert_eq!(patch.status, 200, "{}", patch.text());
    assert_eq!(
        patch.json()["access"]["public_origins"],
        json!(["https://gw.example.com", "http://192.168.1.10:6565"])
    );

    // 普通用户可以读取（但改不了 settings）。
    let created = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/users",
                &json!({"username": "access-user", "password": "access-pass-123", "role": "user"}),
            )
            .header("cookie", &admin),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let login = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({"username": "access-user", "password": "access-pass-123"}),
        ))
        .await;
    assert_eq!(login.status, 200, "{}", login.text());
    let cookie = login
        .header("set-cookie")
        .expect("set-cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();

    let read = harness
        .client
        .send(ClientRequest::new("GET", "/api/access").header("cookie", &cookie))
        .await;
    assert_eq!(read.status, 200, "{}", read.text());
    assert_eq!(
        read.json()["public_origins"],
        json!(["https://gw.example.com", "http://192.168.1.10:6565"])
    );

    // 非法：缺少 scheme、非 http(s)、含 query、重复。
    for body in [
        br#"{"access":{"public_origins":["gw.example.com"]}}"#.as_slice(),
        br#"{"access":{"public_origins":["ftp://gw.example.com"]}}"#.as_slice(),
        br#"{"access":{"public_origins":["https://gw.example.com?x=1"]}}"#.as_slice(),
        br#"{"access":{"public_origins":["https://gw.example.com","https://gw.example.com/"]}}"#
            .as_slice(),
    ] {
        let bad = harness
            .client
            .send(
                ClientRequest::new("PATCH", "/api/settings")
                    .header("content-type", "application/json")
                    .header("cookie", &admin)
                    .raw_body(body.to_vec()),
            )
            .await;
        assert_eq!(bad.status, 422, "{}", bad.text());
    }

    let unauthenticated = harness
        .client
        .send(ClientRequest::new("GET", "/api/access"))
        .await;
    assert_eq!(unauthenticated.status, 401);
}

#[tokio::test]
async fn api_key_expiry_is_supported() {
    let harness = Harness::two_providers("api-key-expiry", "").await;
    let admin = harness.admin_cookie().await;
    let route_id = create_route(&harness, &admin, "expiry").await;
    let future = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 3600;

    let created = harness
        .client
        .send(
            ClientRequest::post_json(
                &format!("/api/routes/{route_id}/api-keys"),
                &json!({"name": "expiring", "expires_at": future}),
            )
            .header("cookie", &admin),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let body = created.json();
    assert_eq!(body["expires_at"], future);
    let id = body["id"].as_str().unwrap().to_string();

    // JSON null = 清空（永不过期）；字段缺省才是“不修改”。
    let cleared = harness
        .client
        .send(
            ClientRequest::new("PATCH", &format!("/api/api-keys/{id}"))
                .header("content-type", "application/json")
                .header("cookie", &admin)
                .raw_body(br#"{"expires_at":null}"#.to_vec()),
        )
        .await;
    assert_eq!(cleared.status, 200, "{}", cleared.text());
    assert_eq!(cleared.json()["expires_at"], serde_json::Value::Null);

    // 创建时不允许已经过期的 expires_at。
    let past = harness
        .client
        .send(
            ClientRequest::post_json(
                &format!("/api/routes/{route_id}/api-keys"),
                &json!({"expires_at": 1}),
            )
            .header("cookie", &admin),
        )
        .await;
    assert_eq!(past.status, 422, "{}", past.text());
    assert_eq!(past.json()["error"]["code"], "invalid_request");
}

// ---------------------------------------------------------------------------
// 多地址监听配置
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multiple_bind_addresses_are_accepted_and_normalized() {
    let harness = Harness::two_providers("bind-multi", "").await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(
            ClientRequest::new("PATCH", "/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(
                    serde_json::to_vec(&json!({
                        "network": { "bind_addrs": ["127.0.0.1", " 127.0.0.1 ", "192.168.1.10"] }
                    }))
                    .unwrap(),
                ),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(
        body["network"]["bind_addrs"],
        json!(["127.0.0.1", "192.168.1.10"]),
        "应去空白并去重"
    );
    // 只读兼容字段同步为首个地址。
    assert_eq!(body["network"]["bind_addr"], "127.0.0.1");
}

#[tokio::test]
async fn wildcard_cannot_be_combined_with_other_addresses() {
    let harness = Harness::two_providers("bind-wildcard", "").await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(
            ClientRequest::new("PATCH", "/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(
                    serde_json::to_vec(&json!({
                        "network": { "bind_addrs": ["0.0.0.0", "127.0.0.1"] }
                    }))
                    .unwrap(),
                ),
        )
        .await;
    assert_eq!(response.status, 422, "{}", response.text());

    // 单独使用通配符可以。
    let ok = harness
        .client
        .send(
            ClientRequest::new("PATCH", "/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(
                    serde_json::to_vec(&json!({ "network": { "bind_addrs": ["0.0.0.0"] } }))
                        .unwrap(),
                ),
        )
        .await;
    assert_eq!(ok.status, 200, "{}", ok.text());
}

#[tokio::test]
async fn legacy_bind_addr_still_works() {
    let harness = Harness::two_providers("bind-legacy", "").await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(
            ClientRequest::new("PATCH", "/api/settings")
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(
                    serde_json::to_vec(&json!({ "network": { "bind_addr": "127.0.0.1" } }))
                        .unwrap(),
                ),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(
        response.json()["network"]["bind_addrs"],
        json!(["127.0.0.1"])
    );
}

#[tokio::test]
async fn network_addresses_returns_candidates_for_admin_only() {
    let harness = Harness::two_providers("bind-candidates", "").await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(ClientRequest::new("GET", "/api/network/addresses").header("cookie", &cookie))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert!(
        body["loopback"]
            .as_array()
            .is_some_and(|list| !list.is_empty())
    );
    assert!(
        body["wildcard"]
            .as_array()
            .is_some_and(|list| !list.is_empty())
    );
    assert!(body["local"].is_array());
}
