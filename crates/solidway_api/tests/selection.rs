//! 路由选号策略接口与 provider 优先级（管理面）。

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

async fn put_selection(
    harness: &Harness,
    cookie: &str,
    route_id: &str,
    body: serde_json::Value,
) -> common::ClientResponse {
    harness
        .client
        .send(
            ClientRequest::new("PUT", &format!("/api/routes/{route_id}/selection-strategy"))
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .raw_body(serde_json::to_vec(&body).unwrap()),
        )
        .await
}

async fn get_selection(harness: &Harness, cookie: &str, route_id: &str) -> common::ClientResponse {
    harness
        .client
        .send(
            ClientRequest::new("GET", &format!("/api/routes/{route_id}/selection-strategy"))
                .header("cookie", cookie),
        )
        .await
}

#[tokio::test]
async fn default_strategy_is_round_robin() {
    let harness = Harness::two_providers("selection-default", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "s1").await;

    let response = get_selection(&harness, &cookie, &route_id).await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["strategy"], "round_robin");
    assert_eq!(body["config"], json!({}));
    assert_eq!(body["resolved"]["fallback"], false);
    assert_eq!(body["resolved"]["weights"]["uptime"], 0.9);
    assert!(body["providers"].is_array());
}

#[tokio::test]
async fn saving_strategy_takes_effect_immediately() {
    let harness = Harness::two_providers("selection-save", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "s2").await;

    let response = put_selection(
        &harness,
        &cookie,
        &route_id,
        json!({ "strategy": "latency", "config": { "weights": { "latency": 0.95, "uptime": 0.05 } } }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = response.json();
    assert_eq!(body["strategy"], "latency");
    assert_eq!(body["resolved"]["weights"]["latency"], 0.95);
    assert_eq!(body["resolved"]["weights"]["uptime"], 0.05);
    // 不要求重启。
    assert!(body.get("restart_required").is_none());

    // 重新读回一致。
    let again = get_selection(&harness, &cookie, &route_id).await.json();
    assert_eq!(again["strategy"], "latency");
    assert_eq!(again["config"]["weights"]["latency"], 0.95);
}

#[tokio::test]
async fn invalid_strategy_and_weights_are_rejected() {
    let harness = Harness::two_providers("selection-invalid", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "s3").await;

    let unknown = put_selection(&harness, &cookie, &route_id, json!({ "strategy": "price" })).await;
    assert_eq!(unknown.status, 422, "{}", unknown.text());

    let price = put_selection(
        &harness,
        &cookie,
        &route_id,
        json!({ "strategy": "auto", "config": { "weights": { "price": 0.3 } } }),
    )
    .await;
    assert_eq!(price.status, 422, "{}", price.text());

    let range = put_selection(
        &harness,
        &cookie,
        &route_id,
        json!({ "strategy": "auto", "config": { "weights": { "latency": 1.5 } } }),
    )
    .await;
    assert_eq!(range.status, 422, "{}", range.text());

    let exploration = put_selection(
        &harness,
        &cookie,
        &route_id,
        json!({ "strategy": "auto", "config": { "thresholds": { "exploration_rate": 2 } } }),
    )
    .await;
    assert_eq!(exploration.status, 422, "{}", exploration.text());
}

#[tokio::test]
async fn zero_weights_are_allowed_and_report_fallback() {
    let harness = Harness::two_providers("selection-fallback", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "s4").await;

    let response = put_selection(
        &harness,
        &cookie,
        &route_id,
        json!({ "strategy": "auto", "config": { "weights": { "uptime": 0, "latency": 0 } } }),
    )
    .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["resolved"]["fallback"], true);
}

#[tokio::test]
async fn another_users_route_is_not_found() {
    let harness = Harness::two_providers("selection-owner", "").await;
    let admin = harness.admin_cookie().await;
    let route_id = create_route(&harness, &admin, "s5").await;

    // 建一个普通用户并登录：路由是 owner 私有资源，他人访问应为 404。
    let created_user = harness
        .client
        .send(
            ClientRequest::post_json(
                "/api/users",
                &json!({"username": "bob", "password": "password123", "role": "user"}),
            )
            .header("cookie", &admin),
        )
        .await;
    assert_eq!(created_user.status, 201, "{}", created_user.text());
    let login = harness
        .client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({"username": "bob", "password": "password123"}),
        ))
        .await;
    assert_eq!(login.status, 200, "{}", login.text());
    let user_cookie = login
        .header("set-cookie")
        .expect("set-cookie")
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let response = get_selection(&harness, &user_cookie, &route_id).await;
    assert_eq!(response.status, 404);

    let response = put_selection(&harness, &user_cookie, &route_id, json!({"strategy": "auto"})).await;
    assert_eq!(response.status, 404);
}

#[tokio::test]
async fn provider_priority_is_validated_and_stored() {
    let harness = Harness::two_providers("selection-priority", "").await;
    let cookie = harness.admin_cookie().await;
    let route_id = create_route(&harness, &cookie, "s6").await;

    let created = harness
        .client
        .send(
            ClientRequest::post_json(
                &format!("/api/routes/{route_id}/provider-keys"),
                &json!({
                    "provider": "openai-custom",
                    "api_key": "sk-test-priority",
                    "base_url": "https://up.example.com/v1",
                    "models": [{"name": "gpt-5"}],
                    "priority": 2,
                }),
            )
            .header("cookie", &cookie),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    assert_eq!(created.json()["priority"], 2);

    let provider_id = created.json()["id"].as_str().unwrap().to_string();
    let patched = harness
        .client
        .send(
            ClientRequest::new("PATCH", &format!("/api/provider-keys/{provider_id}"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(serde_json::to_vec(&json!({"priority": 3})).unwrap()),
        )
        .await;
    assert_eq!(patched.status, 200, "{}", patched.text());
    assert_eq!(patched.json()["priority"], 3);

    // 0 不再是合法档位（不参与选号请用停用），负数与越界同样拒绝。
    for bad in [0, -1, 1001] {
        let response = harness
            .client
            .send(
                ClientRequest::new("PATCH", &format!("/api/provider-keys/{provider_id}"))
                    .header("content-type", "application/json")
                    .header("cookie", &cookie)
                    .raw_body(serde_json::to_vec(&json!({"priority": bad})).unwrap()),
            )
            .await;
        assert_eq!(response.status, 422, "priority={bad}：{}", response.text());
    }

    // 停用即退出选号（不依赖档位）。
    let disabled = harness
        .client
        .send(
            ClientRequest::new("PATCH", &format!("/api/provider-keys/{provider_id}"))
                .header("content-type", "application/json")
                .header("cookie", &cookie)
                .raw_body(serde_json::to_vec(&json!({"status": "inactive"})).unwrap()),
        )
        .await;
    assert_eq!(disabled.status, 200, "{}", disabled.text());
    assert_eq!(disabled.json()["status"], "inactive");
}
