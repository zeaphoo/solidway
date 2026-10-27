//! 路由私有归属：普通用户与 admin 的路由 / API Key 作用域。

mod common;

use common::{ADMIN_PASSWORD, ClientRequest, Harness, TENANT_KEY};
use serde_json::json;

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

#[tokio::test]
async fn routes_and_keys_are_private_to_their_owner() {
    let harness = Harness::two_providers("ownership", "").await;
    let admin_cookie = harness.admin_cookie().await;
    let user_id = create_user(&harness, &admin_cookie, "alice").await;
    let user_cookie = login(&harness, "alice", "user-test-pass").await;

    // 用户创建与 admin 同名路由：允许。
    let created = harness
        .client
        .send(
            ClientRequest::post_json("/api/routes", &json!({ "name": "main" }))
                .header("cookie", &user_cookie),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());
    let user_route_id = created.json()["id"].as_str().expect("route id").to_string();
    assert_eq!(created.json()["owner_id"].as_str(), Some(user_id.as_str()));

    // admin 列表只有自己的 r-main。
    let admin_routes = harness
        .client
        .send(ClientRequest::new("GET", "/api/routes").header("cookie", &admin_cookie))
        .await
        .json();
    let admin_ids: Vec<&str> = admin_routes
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert_eq!(admin_ids, vec!["r-main"]);

    // 用户列表只有自己的路由。
    let user_routes = harness
        .client
        .send(ClientRequest::new("GET", "/api/routes").header("cookie", &user_cookie))
        .await
        .json();
    let user_ids: Vec<&str> = user_routes
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert_eq!(user_ids, vec![user_route_id.as_str()]);

    // 跨用户读取一律 404。
    for (cookie, route_id) in [
        (&admin_cookie, user_route_id.as_str()),
        (&user_cookie, "r-main"),
    ] {
        let response = harness
            .client
            .send(
                ClientRequest::new("GET", &format!("/api/routes/{route_id}"))
                    .header("cookie", cookie),
            )
            .await;
        assert_eq!(response.status, 404);
    }

    // API Key 列表同样隔离。
    let admin_keys_response = harness
        .client
        .send(ClientRequest::new("GET", "/api/api-keys").header("cookie", &admin_cookie))
        .await;
    assert_eq!(
        admin_keys_response.status,
        200,
        "{}",
        admin_keys_response.text()
    );
    let admin_keys = admin_keys_response.json();
    assert_eq!(admin_keys.as_array().unwrap().len(), 1, "{admin_keys}");
    assert_eq!(admin_keys[0]["key_prefix"], "gw-tenan");

    let user_keys = harness
        .client
        .send(ClientRequest::new("GET", "/api/api-keys").header("cookie", &user_cookie))
        .await
        .json();
    assert_eq!(user_keys.as_array().unwrap().len(), 0);

    // 普通用户不能通过统计接口查看他人路由。
    let cross_usage = harness
        .client
        .send(
            ClientRequest::new("GET", "/api/usage/summary?range=today&route_id=r-main")
                .header("cookie", &user_cookie),
        )
        .await;
    assert_eq!(cross_usage.status, 404);

    // 普通用户不能看全局统计。
    let forbidden = harness
        .client
        .send(ClientRequest::new("GET", "/api/admin/usage/summary").header("cookie", &user_cookie))
        .await;
    assert_eq!(forbidden.status, 403);

    // 仍有路由的用户不能删除。
    let delete = harness
        .client
        .send(
            ClientRequest::new("DELETE", &format!("/api/users/{user_id}"))
                .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(delete.status, 409, "{}", delete.text());

    // 清理：删除用户的路由后才能删除用户。
    let deleted_route = harness
        .client
        .send(
            ClientRequest::new("DELETE", &format!("/api/routes/{user_route_id}"))
                .header("cookie", &user_cookie),
        )
        .await;
    assert_eq!(deleted_route.status, 204);
    let delete = harness
        .client
        .send(
            ClientRequest::new("DELETE", &format!("/api/users/{user_id}"))
                .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(delete.status, 204, "{}", delete.text());
}

#[tokio::test]
async fn historical_routes_are_backfilled_to_earliest_admin() {
    let harness = Harness::two_providers("ownership-backfill", "").await;
    let state = harness.api.state();
    let admin_id: String = sqlx::query_scalar(
        "SELECT id FROM users WHERE role = 'admin' ORDER BY created_at, id LIMIT 1",
    )
    .fetch_one(&state.pool)
    .await
    .unwrap();
    let now = solidway_api::state::now_unix();
    let route_id = solidway_api::state::new_id();
    solidway_api::db::routes::create(
        &state.pool,
        &solidway_api::db::routes::Route {
            id: route_id.clone(),
            owner_id: None,
            name: "legacy".to_string(),
            description: None,
            enabled: 1,
            created_at: now,
            updated_at: now,
            selection_strategy: "round_robin".to_string(),
            selection_config: "{}".to_string(),
            archived_at: None,
            archived_by: None,
        },
    )
    .await
    .unwrap();

    let updated = solidway_api::db::routes::backfill_owners(&state.pool)
        .await
        .unwrap();
    assert_eq!(updated, 1);
    let route = solidway_api::db::routes::get(&state.pool, &route_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(route.owner_id.as_deref(), Some(admin_id.as_str()));
}

#[tokio::test]
async fn admin_usage_is_admin_only() {
    let harness = Harness::two_providers("ownership-admin-usage", "").await;
    let admin_cookie = harness.admin_cookie().await;
    // 普通用户登录需要先创建；这里复用 admin 会话验证全局接口可用。
    let response = harness
        .client
        .send(
            ClientRequest::new("GET", "/api/admin/usage/summary?range=today")
                .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    assert_eq!(response.json()["scope"]["kind"], "global");

    // 无 cookie 必须 401。
    let unauth = harness
        .client
        .send(ClientRequest::new("GET", "/api/admin/usage/summary"))
        .await;
    assert_eq!(unauth.status, 401);
    let _ = (TENANT_KEY, ADMIN_PASSWORD);
}
