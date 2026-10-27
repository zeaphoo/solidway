//! 转发主链路集成测试：路由与错误链、保真透传、池模型清单、选号与换号预算。
//!
//! 对应 tasks 3.1–3.3 与 request-forwarding spec 的各 Scenario。

mod common;

use std::time::Duration;

use common::{ClientRequest, ClientResponse, Harness, MockResponse};
use serde_json::json;

const TENANT_KEY: &str = "gw-tenant";

fn chat_request(model: &str) -> serde_json::Value {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": "hello"}],
        "stream": false
    })
}

/// 指向某池的 chat completions 请求。
fn post_chat(_pool: &str, body: &serde_json::Value) -> ClientRequest {
    ClientRequest::post_json(&format!("/v1/chat/completions"), body).bearer(TENANT_KEY)
}

/// 多租户模板里的请求（租户密钥可变）。
fn post_chat_with_key(_pool: &str, key: &str, body: &serde_json::Value) -> ClientRequest {
    ClientRequest::post_json(&format!("/v1/chat/completions"), body).bearer(key)
}

async fn send(harness: &Harness, request: ClientRequest) -> ClientResponse {
    harness.client.send(request).await
}

/// 读响应体里的网关自造错误码。
fn error_code(response: &ClientResponse) -> Option<String> {
    response
        .json()
        .pointer("/error/code")
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

/// 一个池内两个 provider：`gpt-5` 与 `gpt-5-mini` 的候选集合都是 [p-a, p-b]。
async fn harness(tag: &str, extra_toml: &str) -> Harness {
    let harness = Harness::two_providers(tag, extra_toml).await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));
    harness
}

// ---------------------------------------------------------------------------
// 鉴权与端点分类
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invalid_tenant_key_returns_gateway_401() {
    let harness = harness("auth", "").await;
    let response = send(
        &harness,
        post_chat_with_key("main", "gw-wrong", &chat_request("gpt-5")),
    )
    .await;
    assert_eq!(response.status, 401);
    assert_eq!(error_code(&response).as_deref(), Some("invalid_api_key"));
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "无效凭据不得转发"
    );
}

#[tokio::test]
async fn missing_credentials_are_rejected_without_forwarding() {
    let harness = harness("nokey", "").await;
    let response = send(
        &harness,
        ClientRequest::post_json("/v1/chat/completions", &chat_request("gpt-5")),
    )
    .await;
    assert_eq!(response.status, 401);
    assert_eq!(harness.mocks.a.count() + harness.mocks.b.count(), 0);
}

#[tokio::test]
async fn invalid_credentials_win_over_every_endpoint_class() {
    // 鉴权先于端点分类：组合场景必须 401 而不是 404（request-forwarding spec）。
    let harness = harness("auth-first", "").await;
    for (method, path) in [
        ("GET", "/v1/files"),
        ("GET", "/v1/models"),
        ("POST", "/v1/models"),
        ("POST", "/v1/chat/completions"),
        ("GET", "/v1/models"),
        ("GET", "/v1/unknown"),
    ] {
        // 注：非 `/v1` 的 path 不属于网关范围（由管理面/静态托管处理），不参与本条断言。
        let response = send(
            &harness,
            ClientRequest::new(method, path).bearer("gw-wrong"),
        )
        .await;
        assert_eq!(response.status, 401, "{method} {path} 应回 401");
        assert_eq!(
            error_code(&response).as_deref(),
            Some("invalid_api_key"),
            "{method} {path} 的错误码应为 invalid_api_key"
        );
    }
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "鉴权失败不得转发"
    );
}

#[tokio::test]
async fn old_paths_and_unknown_paths_return_404_unsupported_endpoint() {
    let harness = harness("unsupported", "").await;

    // 工作端点已改为 /v1/chat/completions、/v1/responses、/v1/models；其余 `/v1/*` 一律 404。
    for (method, path) in [
        ("GET", "/v1/files"),
        ("GET", "/v1/responses/resp_123"),
        ("GET", "/v1/chat/completions/extra"),
        ("GET", "/v1/models/extra"),
        ("GET", "/v1"),
        ("GET", "/v1/unknown"),
    ] {
        let response = send(
            &harness,
            ClientRequest::new(method, path).bearer(TENANT_KEY),
        )
        .await;
        assert_eq!(response.status, 404, "{method} {path} 必须被拒");
        assert_eq!(
            error_code(&response).as_deref(),
            Some("unsupported_endpoint"),
            "{method} {path} 的错误码应为 unsupported_endpoint"
        );
    }
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "非工作端点绝不能转发上游"
    );
}

#[tokio::test]
async fn unsupported_endpoint_is_rejected_before_reading_the_body() {
    // 端点分类先于读体：声明 1MB 请求体但一个字节都不发，网关也必须立刻作答（404）。
    // 若它先读体，这里将拿不到任何响应——这同时证明 404 优先于 413。
    let harness = harness("no-read", "").await;
    let text = raw_request(
        &harness,
        "POST /v1/files HTTP/1.1\r\nhost: gateway.local\r\nconnection: close\r\n\
         authorization: Bearer gw-tenant\r\ncontent-type: application/json\r\n\
         content-length: 1048576\r\n\r\n",
        "unsupported_endpoint",
    )
    .await;
    assert!(
        text.starts_with("HTTP/1.1 404"),
        "应直接回 404（实际响应：{text}）"
    );
    assert!(
        text.contains("unsupported_endpoint"),
        "错误码应为 unsupported_endpoint（实际响应：{text}）"
    );
}

// ---------------------------------------------------------------------------
// 池定位（404 pool_not_found）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_endpoint_returns_404_unsupported_endpoint() {
    // 本项目寻址为 key→route：path 不再携带池标识，未定义 path 一律 404。
    let harness = harness("ghost-path", "").await;
    let response = send(
        &harness,
        ClientRequest::post_json("/v1/ghost/chat/completions", &chat_request("gpt-5"))
            .bearer(TENANT_KEY),
    )
    .await;
    assert_eq!(response.status, 404);
    assert_eq!(error_code(&response).as_deref(), Some("unsupported_endpoint"));
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "未定义端点不得转发"
    );
}

#[tokio::test]
async fn unknown_endpoint_is_decided_before_reading_the_body() {
    // 端点分类先于读体：未定义 path 即使声明大请求体也必须立刻 404（不得 413）。
    let harness = harness("ghost-noread", "").await;
    let text = raw_request(
        &harness,
        "POST /v1/ghost/chat/completions HTTP/1.1\r\nhost: gateway.local\r\n\
         connection: close\r\nauthorization: Bearer gw-tenant\r\n\
         content-type: application/json\r\ncontent-length: 1048576\r\n\r\n",
        "unsupported_endpoint",
    )
    .await;
    assert!(
        text.starts_with("HTTP/1.1 404"),
        "应直接回 404（实际响应：{text}）"
    );
    assert!(
        text.contains("unsupported_endpoint"),
        "错误码应为 unsupported_endpoint（实际响应：{text}）"
    );
    assert!(
        !text.contains("request_too_large"),
        "端点分类失败不得整读请求体（不得回 413）：{text}"
    );
}

#[tokio::test]
async fn unknown_paths_are_indistinguishable_and_not_forwarded() {
    // 不泄露内部结构：任意两个未定义 path 的响应必须逐字节一致。
    let harness = harness("cross-path", "").await;
    let a = send(
        &harness,
        ClientRequest::post_json("/v1/alpha", &chat_request("gpt-5")).bearer(TENANT_KEY),
    )
    .await;
    let b = send(
        &harness,
        ClientRequest::post_json("/v1/beta", &chat_request("gpt-5")).bearer(TENANT_KEY),
    )
    .await;
    assert_eq!(a.status, 404);
    assert_eq!(b.status, 404);
    assert_eq!(a.body, b.body, "未定义 path 的响应必须一致");
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count() + harness.mocks.c.count(),
        0
    );
}

// ---------------------------------------------------------------------------
// 池模型清单端点（GET /v1/models）
// ---------------------------------------------------------------------------

#[tokio::test]
async fn models_endpoint_lists_the_pool_model_union() {
    let harness = harness("models", "").await;

    let response = send(
        &harness,
        ClientRequest::new("GET", "/v1/models").bearer(TENANT_KEY),
    )
    .await;
    assert_eq!(response.status, 200);
    let body = response.json();
    assert_eq!(body.get("object").and_then(|v| v.as_str()), Some("list"));
    let entries = body
        .get("data")
        .and_then(|v| v.as_array())
        .expect("data 数组");
    let ids: Vec<&str> = entries
        .iter()
        .filter_map(|entry| entry.get("id").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(ids, vec!["gpt-5", "gpt-5-mini"], "并集按字典序输出");
    for entry in entries {
        assert_eq!(entry.get("object").and_then(|v| v.as_str()), Some("model"));
        assert_eq!(entry.get("created").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(
            entry.get("owned_by").and_then(|v| v.as_str()),
            Some("main"),
            "owned_by 为池名"
        );
    }
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "模型清单必须本地合成，不转发上游"
    );
}

#[tokio::test]
async fn models_endpoint_rejects_non_get_with_405_and_allow_header() {
    let harness = harness("models-405", "").await;
    for method in ["POST", "PUT", "DELETE"] {
        let response = send(
            &harness,
            ClientRequest::new(method, "/v1/models").bearer(TENANT_KEY),
        )
        .await;
        assert_eq!(response.status, 405, "{method} 应回 405");
        assert_eq!(
            response.header("allow"),
            Some("GET"),
            "{method} 的 405 必须携带 Allow 头"
        );
    }
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "405 不得转发上游"
    );
}

#[tokio::test]
async fn models_endpoint_non_get_returns_405_after_auth() {
    // 顺序：鉴权（401）→ method 路由（405）→ 端点分类（404）。
    let harness = harness("models-n5", "").await;

    let post = send(
        &harness,
        ClientRequest::new("POST", "/v1/models").bearer(TENANT_KEY),
    )
    .await;
    assert_eq!(post.status, 405);
    assert_eq!(post.header("allow"), Some("GET"));

    // 无效凭据优先于 method：同样是 POST，无凭据时是 401。
    let unauthorized = send(&harness, ClientRequest::new("POST", "/v1/models")).await;
    assert_eq!(unauthorized.status, 401, "鉴权先于 method 路由");
    assert_eq!(harness.mocks.a.count() + harness.mocks.b.count(), 0);
}

#[tokio::test]
async fn models_endpoint_only_lists_its_own_route() {
    // 每个 API Key 只定位自己的路由；/v1/models 不得暴露其它路由的模型。
    let harness = common::build_harness("models-scope", "", "/v1", |m| {
        vec![
            common::SeedRoute::new("r1", "one")
                .provider("p-a", m.a.base_url(), &["gpt-5"])
                .key("k1", "sw-one"),
            common::SeedRoute::new("r2", "two")
                .provider("p-b", m.b.base_url(), &["gpt-4o"])
                .key("k2", "sw-two"),
        ]
    })
    .await;

    let one = send(
        &harness,
        ClientRequest::new("GET", "/v1/models").bearer("sw-one"),
    )
    .await;
    assert_eq!(one.status, 200);
    let ids: Vec<String> = one.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["gpt-5"]);

    let two = send(
        &harness,
        ClientRequest::new("GET", "/v1/models").bearer("sw-two"),
    )
    .await;
    let ids: Vec<String> = two.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["gpt-4o"]);
}

// ---------------------------------------------------------------------------
// model 定位（404 model_not_found）与保真透传
// ---------------------------------------------------------------------------

#[tokio::test]
async fn model_not_in_pool_union_returns_404_model_not_found() {
    let harness = harness("nomap", "").await;
    let response = send(&harness, post_chat("main", &chat_request("gpt-unknown"))).await;
    assert_eq!(response.status, 404, "model 不在池并集必须由网关自造 404");
    assert_eq!(error_code(&response).as_deref(), Some("model_not_found"));
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "路由未命中不得转发上游"
    );
}

#[tokio::test]
async fn missing_or_unparsable_model_returns_404_model_not_found() {
    let harness = harness("badmodel", "").await;

    let bodies: Vec<Vec<u8>> = vec![
        serde_json::to_vec(&json!({"messages": []})).expect("json"),
        br#"{"model": 42}"#.to_vec(),
        b"not json at all".to_vec(),
    ];
    for body in bodies {
        let response = send(
            &harness,
            ClientRequest::new("POST", "/v1/chat/completions")
                .bearer(TENANT_KEY)
                .header("content-type", "application/json")
                .raw_body(body.clone()),
        )
        .await;
        assert_eq!(
            response.status,
            404,
            "请求体 {:?} 应回 404",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(
            error_code(&response).as_deref(),
            Some("model_not_found"),
            "请求体 {:?} 的错误码应为 model_not_found",
            String::from_utf8_lossy(&body)
        );
    }
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "无法路由的请求不得转发上游"
    );
}

#[tokio::test]
async fn request_body_reaches_upstream_byte_for_byte() {
    // 保真红线：白名单只剩鉴权头与目标地址，请求体 MUST 逐字节原样转发（model 不再改写）。
    let harness = harness("verbatim-body", "").await;
    let raw_body = br#"{
        "model": "gpt-5",
        "messages": [ {"role": "user", "content": "hi"} ],
        "unknown_beta": {"nested": [1, 2, 3]}
    }"#
    .to_vec();

    let response = send(
        &harness,
        ClientRequest::new("POST", "/v1/chat/completions")
            .bearer(TENANT_KEY)
            .header("content-type", "application/json")
            .raw_body(raw_body.clone()),
    )
    .await;
    assert_eq!(response.status, 200);

    let request = harness
        .mocks
        .a
        .last_request()
        .or_else(|| harness.mocks.b.last_request())
        .expect("上游应收到请求");
    assert_eq!(
        request.body, raw_body,
        "请求体必须逐字节一致（含未知字段与空白）"
    );
    assert_eq!(
        request.json().get("model").and_then(|value| value.as_str()),
        Some("gpt-5"),
        "model 字段原样透传"
    );
}

#[tokio::test]
async fn unknown_fields_and_custom_headers_reach_upstream() {
    let harness = harness("fidelity", "").await;

    let body = json!({
        "model": "gpt-5",
        "messages": [{"role": "user", "content": "hi"}],
        "beta_feature": {"nested": [1, 2, 3]},
        "custom_string": "untouched"
    });
    let response = send(
        &harness,
        post_chat("main", &body).header("x-custom-client-header", "keep-me"),
    )
    .await;
    assert_eq!(response.status, 200);

    let upstream = harness
        .mocks
        .a
        .last_request()
        .or_else(|| harness.mocks.b.last_request())
        .expect("上游应收到请求");
    let received = upstream.json();
    assert_eq!(
        received.get("beta_feature"),
        body.get("beta_feature"),
        "未知字段必须原样到达上游"
    );
    assert_eq!(received.get("custom_string"), body.get("custom_string"));
    assert_eq!(
        upstream.header("x-custom-client-header"),
        Some("keep-me"),
        "自定义头必须透传"
    );
}

#[tokio::test]
async fn upstream_response_is_returned_verbatim() {
    let harness = harness("verbatim", "").await;
    harness
        .mocks
        .a
        .set_fallback(MockResponse::ok("gpt-5").with_header("x-upstream-marker", "marker-value"));
    harness
        .mocks
        .b
        .set_fallback(MockResponse::ok("gpt-5").with_header("x-upstream-marker", "marker-value"));

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.header("x-upstream-marker"), Some("marker-value"));
    assert_eq!(
        response.json().get("id").and_then(|v| v.as_str()),
        Some("chatcmpl-mock")
    );
}

#[tokio::test]
async fn authorization_is_replaced_with_provider_key() {
    let harness = harness("authz", "").await;

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 200);

    let (upstream, expected_key) = match harness.mocks.a.last_request() {
        Some(request) => (request, "sk-provider-a"),
        None => (
            harness
                .mocks
                .b
                .last_request()
                .expect("某个 provider 应收到请求"),
            "sk-provider-b",
        ),
    };
    assert_eq!(
        upstream.header("authorization"),
        Some(format!("Bearer {expected_key}").as_str()),
        "上游收到的必须是该 provider 的 API Key"
    );
    assert!(
        !upstream
            .header("authorization")
            .unwrap_or_default()
            .contains(TENANT_KEY),
        "租户密钥不得出现在上游请求里"
    );
}

#[tokio::test]
async fn target_url_keeps_base_path_prefix() {
    // base URL 带路径前缀时，转发目标保留该前缀，仅剥掉入站 path 的 /v1。
    let harness = common::build_harness("prefix", "", "/compatible-mode/v1", |m| {
        vec![
            common::SeedRoute::new("r-main", "main")
                .provider("p-a", m.a.base_url(), &["gpt-5", "gpt-5-mini"])
                .provider("p-b", m.b.base_url(), &["gpt-5", "gpt-5-mini"])
                .key("k-main", TENANT_KEY),
        ]
    })
    .await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 200);
    let request = harness
        .mocks
        .a
        .last_request()
        .or_else(|| harness.mocks.b.last_request())
        .expect("上游应收到请求");
    assert_eq!(
        request.path, "/compatible-mode/v1/chat/completions",
        "base URL 的路径前缀必须保留"
    );
}

#[tokio::test]
async fn query_string_is_passed_through_verbatim() {
    let harness = harness("query", "").await;

    let response = send(
        &harness,
        ClientRequest::post_json(
            "/v1/responses?foo=bar&baz=1",
            &chat_request("gpt-5"),
        )
        .bearer(TENANT_KEY),
    )
    .await;
    assert_eq!(response.status, 200);
    let request = harness
        .mocks
        .a
        .last_request()
        .or_else(|| harness.mocks.b.last_request())
        .expect("上游应收到请求");
    assert_eq!(
        request.path, "/v1/responses",
        "入站 path 去掉 /pools/{{pool}}/v1 前缀后接到 base URL 之后"
    );
    assert_eq!(request.query, "foo=bar&baz=1", "query 必须原样透传");
}

#[tokio::test]
async fn health_endpoint_is_local_and_never_forwarded() {
    let harness = harness("health", "").await;
    // /health 免鉴权：无凭据也本地应答（先于一切）。
    let response = send(&harness, ClientRequest::new("GET", "/health")).await;
    assert_eq!(response.status, 200);
    assert_eq!(response.text(), "ok");
    let response_with_key = send(
        &harness,
        ClientRequest::new("GET", "/health").bearer(TENANT_KEY),
    )
    .await;
    assert_eq!(response_with_key.status, 200);
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "/health 绝不能转发上游"
    );
}

#[tokio::test]
async fn health_endpoint_non_get_returns_405() {
    let harness = harness("health-405", "").await;
    let response = send(&harness, ClientRequest::new("POST", "/health")).await;
    assert_eq!(response.status, 405, "非 GET 的 /health 由 method 路由应答");
    assert!(
        response
            .header("allow")
            .is_some_and(|value| value.contains("GET")),
        "405 必须携带 Allow 头：{:?}",
        response.header("allow")
    );
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "405 不得转发上游"
    );
}

// ---------------------------------------------------------------------------
// 多租户 / 多池路由
// ---------------------------------------------------------------------------

#[tokio::test]
async fn one_api_key_reaches_all_models_of_its_route() {
    // 一把 API Key 定位一个路由，可访问该路由的全部模型（并集）；model 原样透传。
    let harness = harness("multi-model", "").await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let models = send(
        &harness,
        ClientRequest::new("GET", "/v1/models").bearer(TENANT_KEY),
    )
    .await;
    assert_eq!(models.status, 200);
    let mut ids: Vec<String> = models.json()["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    assert_eq!(ids, ["gpt-5", "gpt-5-mini"]);

    for model in ["gpt-5", "gpt-5-mini"] {
        let chat = send(&harness, post_chat("main", &chat_request(model))).await;
        assert_eq!(chat.status, 200, "该路由的模型 {model} 应可路由");
    }
}

#[tokio::test]
async fn each_api_key_locates_its_own_route() {
    let harness = common::build_harness("key-route", "", "/v1", |m| {
        vec![
            common::SeedRoute::new("r1", "one")
                .provider("p-a", m.a.base_url(), &["gpt-5"])
                .key("k1", "sw-one"),
            common::SeedRoute::new("r2", "two")
                .provider("p-b", m.b.base_url(), &["gpt-3.5"])
                .key("k2", "sw-two"),
        ]
    })
    .await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-3.5"));

    let one = send(
        &harness,
        ClientRequest::post_json("/v1/chat/completions", &chat_request("gpt-5")).bearer("sw-one"),
    )
    .await;
    assert_eq!(one.status, 200);
    assert_eq!(harness.mocks.a.count(), 1);

    let two = send(
        &harness,
        ClientRequest::post_json("/v1/chat/completions", &chat_request("gpt-3.5")).bearer("sw-two"),
    )
    .await;
    assert_eq!(two.status, 200);
    assert_eq!(harness.mocks.b.count(), 1);

    // 交叉：sw-one 不能访问 r2 的模型。
    let cross = send(
        &harness,
        ClientRequest::post_json("/v1/chat/completions", &chat_request("gpt-3.5")).bearer("sw-one"),
    )
    .await;
    assert_eq!(cross.status, 404);
    assert_eq!(error_code(&cross).as_deref(), Some("model_not_found"));
}

// ---------------------------------------------------------------------------
// 选号与候选集合
// ---------------------------------------------------------------------------

#[tokio::test]
async fn round_robin_distributes_evenly_across_healthy_providers() {
    let harness = harness("rr", "").await;

    for _ in 0..4 {
        let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
        assert_eq!(response.status, 200, "全部请求都应成功");
    }
    assert_eq!(
        harness.mocks.a.count(),
        2,
        "2 个健康 provider 应各承载 2 次"
    );
    assert_eq!(harness.mocks.b.count(), 2);
}

#[tokio::test]
async fn round_robin_counters_are_independent_per_model() {
    let harness = harness("rr-per-model", "").await;

    // 两个模型的计数器各自从 0 开始：首个请求都落在 p-a。
    let first = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(first.status, 200);
    let second = send(&harness, post_chat("main", &chat_request("gpt-5-mini"))).await;
    assert_eq!(second.status, 200);

    assert_eq!(
        harness.mocks.a.count(),
        2,
        "按 (池, 模型) 独立计数时，两个模型的首个请求都落在 p-a"
    );
    assert_eq!(
        harness.mocks.b.count(),
        0,
        "共享计数器才会把第二个模型的请求推到 p-b"
    );
}

#[tokio::test]
async fn unhealthy_providers_are_skipped() {
    let harness = harness("skip", "").await;

    // provider A 先返回 401 → 临时移出（冷却 30m，不会回池）。
    harness.mocks.a.script(vec![MockResponse::unauthorized()]);
    let first = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(first.status, 200, "换号后客户端应看到成功");
    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1);

    // 后续请求只应打到 B。
    for _ in 0..3 {
        let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(harness.mocks.a.count(), 1, "被移出的 provider 不得再承载");
    assert_eq!(harness.mocks.b.count(), 4);
}

// ---------------------------------------------------------------------------
// 请求体读取上限与读取失败
// ---------------------------------------------------------------------------

#[tokio::test]
async fn oversized_buffered_body_returns_413() {
    let harness = harness("toolarge", "").await;
    // 32MB + 1：超过整读上限，网关必须在转发前拒绝。
    let mut body = br#"{"model":"gpt-5","pad":""#.to_vec();
    body.resize(32 * 1024 * 1024 + 1, b'a');
    body.extend_from_slice(br#""}"#);

    let response = send(
        &harness,
        ClientRequest::new("POST", "/v1/chat/completions")
            .bearer(TENANT_KEY)
            .header("content-type", "application/json")
            .raw_body(body),
    )
    .await;
    assert_eq!(response.status, 413);
    assert_eq!(error_code(&response).as_deref(), Some("request_too_large"));
    assert_eq!(harness.mocks.a.count() + harness.mocks.b.count(), 0);
}

#[tokio::test]
async fn incomplete_request_body_returns_400_request_body_incomplete() {
    // 声明 100 字节请求体、只发一部分即关闭写方向：读取中途失败 →
    // 400 request_body_incomplete，且不转发上游（request-forwarding「网关自造错误体格式」）。
    let harness = harness("body-incomplete", "").await;

    let text = raw_request_with_partial_body(
        &harness,
        "POST /v1/chat/completions HTTP/1.1\r\nhost: gateway.local\r\n\
         connection: close\r\nauthorization: Bearer gw-tenant\r\n\
         content-type: application/json\r\ncontent-length: 100\r\n\r\n",
        br#"{"model":"gpt-5","messages":[]}"#,
        "request_body_incomplete",
    )
    .await;
    assert!(
        text.starts_with("HTTP/1.1 400"),
        "应回 400（实际响应：{text}）"
    );
    assert!(
        text.contains("request_body_incomplete"),
        "错误码应为 request_body_incomplete（实际响应：{text}）"
    );
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        0,
        "请求体读取失败不得转发上游"
    );
}

// ---------------------------------------------------------------------------
// 换号与重试预算
// ---------------------------------------------------------------------------

#[tokio::test]
async fn failover_on_429_is_transparent_to_client() {
    let harness = harness("failover", "").await;
    harness
        .mocks
        .a
        .script(vec![MockResponse::rate_limited(None)]);
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 200, "客户端只应看到最终成功响应");
    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1);

    // 换号不改变请求体：发给 B 的 model 仍是客户端原样值（无改写白名单项）。
    let to_b = harness.mocks.b.last_request().expect("换号后应由 B 承载");
    assert_eq!(
        to_b.json().get("model").and_then(|v| v.as_str()),
        Some("gpt-5"),
        "换号后请求体不得改写"
    );
}

#[tokio::test]
async fn all_providers_failing_returns_last_error_verbatim() {
    let harness = harness("allfail", "").await;
    // 带 Retry-After 的 429：回传时必须保留该头。
    harness
        .mocks
        .a
        .script(vec![MockResponse::rate_limited(Some("120"))]);
    harness
        .mocks
        .b
        .script(vec![MockResponse::rate_limited(Some("60"))]);

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 429);
    assert_eq!(
        error_code(&response).as_deref(),
        Some("rate_limit_exceeded"),
        "最后一个错误响应体必须原样返回"
    );
    assert_eq!(
        response.header("retry-after"),
        Some("60"),
        "最后一个错误响应的头必须保留"
    );
    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1, "每个健康 provider 最多尝试一次");
}

#[tokio::test]
async fn each_healthy_provider_is_tried_at_most_once() {
    let harness = harness("budget", "").await;
    harness.mocks.a.script(vec![
        MockResponse::rate_limited(None),
        MockResponse::ok("gpt-5"),
    ]);
    harness.mocks.b.script(vec![
        MockResponse::rate_limited(None),
        MockResponse::ok("gpt-5"),
    ]);

    let first = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(
        first.status, 429,
        "两个 provider 各失败一次后应回传最后错误"
    );
    assert_eq!(harness.mocks.a.count(), 1);
    assert_eq!(harness.mocks.b.count(), 1);
}

#[tokio::test]
async fn request_level_error_does_not_switch_providers() {
    let harness = harness("noretry", "").await;
    harness.mocks.a.set_fallback(MockResponse::bad_request());
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 400);
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        1,
        "请求级错误不得换号"
    );
}

#[tokio::test]
async fn upstream_5xx_causes_failover_then_quarantine() {
    let harness = harness("fivexx", "").await;
    harness.mocks.a.script(vec![MockResponse::overloaded()]);

    let first = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(first.status, 200, "5xx 应换号并对客户端透明");

    // A 进入冷却（默认 1m，不会到期），后续请求不再打它。
    for _ in 0..3 {
        let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
        assert_eq!(response.status, 200);
    }
    assert_eq!(harness.mocks.a.count(), 1, "过载 provider 应被临时移出");
}

#[tokio::test]
async fn stream_failure_after_first_byte_does_not_switch_providers() {
    let harness = harness("midstream", "").await;
    // A 返回「先给头、再延迟」的响应：网关一旦开始回写就关闭换号窗口。
    harness.mocks.a.set_fallback(MockResponse::delayed_body(
        200,
        "first-chunk",
        Duration::from_millis(30),
    ));

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 200);
    assert_eq!(harness.mocks.b.count(), 0, "已开始回写响应后不得再换号");
    assert_eq!(harness.mocks.a.count(), 1);
}

#[tokio::test]
async fn transport_failure_fails_over_and_quarantines_the_provider() {
    // provider A 指向「接受即关闭」的假上游：确定性产生传输层失败。
    let dropping_port = common::start_dropping_upstream().await;
    let harness = common::build_harness("transport", "", "/v1", |m| {
        vec![
            common::SeedRoute::new("r-main", "main")
                .provider(
                    "p-a",
                    format!("http://127.0.0.1:{dropping_port}/v1"),
                    &["gpt-5", "gpt-5-mini"],
                )
                .provider("p-b", m.b.base_url(), &["gpt-5", "gpt-5-mini"])
                .key("k-main", TENANT_KEY),
        ]
    })
    .await;
    harness.mocks.b.set_fallback(MockResponse::ok("gpt-5"));

    let response = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(response.status, 200, "传输失败应换号并对客户端透明");
    assert_eq!(harness.mocks.b.count(), 1, "换号后应由健康 provider 承载");
    let rows = common::wait_for_rows(&harness.db_path, Duration::from_secs(5), |rows| rows.len() == 1).await;
    assert_eq!(rows.len(), 1, "传输失败必须落库：{rows:?}");
    assert_eq!(rows[0].0, "p-a");
    assert_eq!(rows[0].1, "quarantined");
}

#[tokio::test]
async fn pool_exhausted_returns_503() {
    let harness = harness("empty", "").await;
    // 两个 provider 都先被打成 401：第一次请求触发两次换号（全失败），之后候选集合无健康 provider。
    harness.mocks.a.script(vec![MockResponse::unauthorized()]);
    harness.mocks.b.script(vec![MockResponse::unauthorized()]);

    let first = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(first.status, 401, "全失败应返回最后一个上游错误");
    assert_eq!(
        first.header("content-type"),
        Some("application/json"),
        "上游响应头必须保留"
    );

    let second = send(&harness, post_chat("main", &chat_request("gpt-5"))).await;
    assert_eq!(second.status, 503);
    assert_eq!(error_code(&second).as_deref(), Some("route_exhausted"));
    assert_eq!(
        harness.mocks.a.count() + harness.mocks.b.count(),
        2,
        "候选集合耗尽后不得再打上游"
    );
}

// ---------------------------------------------------------------------------
// 流式不缓冲
// ---------------------------------------------------------------------------

#[tokio::test]
async fn streaming_response_is_not_buffered() {
    let harness = harness("stream", "").await;
    let first_event = "data: {\"delta\":\"first\"}\n\n";
    let last_event = "data: [DONE]\n\n";
    harness.mocks.a.set_fallback(MockResponse::sse(
        &[first_event, last_event],
        Duration::from_millis(250),
    ));

    let mut chunks: Vec<Vec<u8>> = Vec::new();
    let started = std::time::Instant::now();
    let response = harness
        .client
        .stream_chunks(post_chat("main", &chat_request("gpt-5")), |chunk| {
            chunks.push(chunk.to_vec());
        })
        .await;
    let elapsed = started.elapsed();

    assert_eq!(response.status, 200);
    assert_eq!(
        response.body,
        format!("{first_event}{last_event}").into_bytes(),
        "SSE 事件必须按序完整转发"
    );
    assert!(
        chunks.len() >= 2,
        "应观察到多个分块（边收边转），实际 {}",
        chunks.len()
    );
    assert!(
        elapsed >= Duration::from_millis(200),
        "第二个事件是延迟下发的：若网关缓冲则会等到全部结束，耗时 {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// 报文级回归：原样转发不得留下失真的长度头 / 重复鉴权头
// ---------------------------------------------------------------------------

#[tokio::test]
async fn raw_wire_has_no_stale_length_or_duplicate_auth() {
    let harness = harness("raw-wire", "").await;

    // 刻意用「带空格 + 注释字段」的 JSON：验证逐字节保真时长度与入站一致。
    let raw_body = br#"{
        "model": "gpt-5",
        "messages": [ {"role": "user", "content": "hi"} ]
    }"#
    .to_vec();
    let inbound_length = raw_body.len();

    let response = send(
        &harness,
        ClientRequest::new("POST", "/v1/chat/completions")
            .bearer(TENANT_KEY)
            .header("content-type", "application/json")
            .raw_body(raw_body.clone()),
    )
    .await;
    assert_eq!(response.status, 200, "原样转发必须仍能正常送达");

    let (provider, raw) = match harness.mocks.a.last_raw() {
        Some(raw) => ("p-a", raw),
        None => (
            "p-b",
            harness.mocks.b.last_raw().expect("上游应收到原始报文"),
        ),
    };
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text.split_once("\r\n\r\n").expect("报文应包含头与体");

    let auth_headers: Vec<&str> = head
        .lines()
        .filter(|line| line.to_ascii_lowercase().starts_with("authorization:"))
        .collect();
    assert_eq!(
        auth_headers.len(),
        1,
        "上游只能收到一个 authorization 头（实际 {auth_headers:?}）"
    );
    assert_eq!(
        auth_headers[0],
        format!(
            "authorization: Bearer sk-provider-{}",
            &provider[provider.len() - 1..]
        ),
        "鉴权头必须是该 provider 的 API Key"
    );
    assert!(
        !head.contains(TENANT_KEY),
        "租户密钥绝不能出现在上游请求里：{head}"
    );
    // 请求体已被整读：长度头必须由本进程按**实际**请求体重建，不得透传客户端的陈旧长度。
    let length_headers: Vec<&str> = head
        .lines()
        .filter(|line| line.to_ascii_lowercase().starts_with("content-length:"))
        .collect();
    assert_eq!(length_headers.len(), 1, "只应有一个 content-length：{head}");
    let declared: usize = length_headers[0]
        .split(':')
        .nth(1)
        .expect("length 值")
        .trim()
        .parse()
        .expect("length 应为十进制");
    assert_eq!(
        declared, inbound_length,
        "长度头必须与入站请求体一致（不得失真）"
    );
    assert_eq!(
        body.as_bytes(),
        raw_body.as_slice(),
        "请求体必须与入站逐字节一致"
    );
}

// ---------------------------------------------------------------------------
// 裸报文辅助
// ---------------------------------------------------------------------------

/// 只发请求头（不发体）并读到某错误码出现为止，返回响应原文。
async fn raw_request(harness: &Harness, head: &str, until: &str) -> String {
    raw_exchange(harness, head, b"", false, until).await
}

/// 发请求头 + 部分体后关闭写方向（入站报文短于声明的 content-length）。
async fn raw_request_with_partial_body(
    harness: &Harness,
    head: &str,
    body: &[u8],
    until: &str,
) -> String {
    raw_exchange(harness, head, body, true, until).await
}

async fn raw_exchange(
    harness: &Harness,
    head: &str,
    body: &[u8],
    shutdown_write: bool,
    until: &str,
) -> String {
    let mut stream =
        tokio::net::TcpStream::connect(harness.base_url.trim_start_matches("http://").to_string())
            .await
            .expect("connect gateway");
    tokio::io::AsyncWriteExt::write_all(&mut stream, head.as_bytes())
        .await
        .expect("write head");
    if !body.is_empty() {
        tokio::io::AsyncWriteExt::write_all(&mut stream, body)
            .await
            .expect("write body");
    }
    tokio::io::AsyncWriteExt::flush(&mut stream)
        .await
        .expect("flush");
    if shutdown_write {
        tokio::io::AsyncWriteExt::shutdown(&mut stream)
            .await
            .expect("shutdown write half");
    }

    let mut buffer = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let read = tokio::io::AsyncReadExt::read_buf(&mut stream, &mut buffer)
                .await
                .expect("read response");
            if read == 0 || String::from_utf8_lossy(&buffer).contains(until) {
                break;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("网关必须在 {until} 场景下作答"));
    String::from_utf8_lossy(&buffer).to_string()
}
