//! 版本号对**任意已认证角色**可见（非 admin 看不到「系统」页）。
//!
//! 对应 `openspec/changes/2026-10-18-version-visible-to-all`：
//! `GET /api/version` 只回版本号；`GET /api/system`、`GET /api/app/info` 仍仅 admin。

mod common;

use common::{ClientRequest, RawClient};
use serde_json::json;
use solidway_api::{Api, ApiConfig, RunMode};

const ADMIN_PASSWORD: &str = "admin-pass-123";
const USER_PASSWORD: &str = "user-pass-123";

async fn start_api(tag: &str) -> (RawClient, Api) {
    let data_root = common::temp_data_root(tag);
    let _ = std::fs::remove_dir_all(&data_root);
    let api = Api::init(ApiConfig {
        data_root,
        master_key: None,
        mode: RunMode::Standalone,
        web_assets: None,
    })
    .await
    .expect("init api");

    // 走一遍引导，把 admin 口令设成已知值。
    let token = api.bootstrap_token().expect("bootstrap token").to_string();
    let app = api.router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let client = RawClient::new(&format!("http://{addr}"));
    let response = client
        .send(ClientRequest::post_json(
            "/api/auth/bootstrap",
            &json!({"token": token, "new_password": ADMIN_PASSWORD}),
        ))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    (client, api)
}

/// 登录并取出 Cookie（直接用作后续请求头）。
async fn login_cookie(client: &RawClient, username: &str, password: &str) -> String {
    let response = client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({"username": username, "password": password}),
        ))
        .await;
    assert_eq!(response.status, 200, "登录失败: {}", response.text());
    let cookie = response.header("set-cookie").expect("set-cookie").to_string();
    cookie.split(';').next().expect("cookie pair").to_string()
}

#[tokio::test]
async fn version_is_visible_to_non_admin_but_system_is_not() {
    let (client, _api) = start_api("version-roles").await;
    let admin_cookie = login_cookie(&client, "admin", ADMIN_PASSWORD).await;

    // admin 建一个普通用户。
    let created = client
        .send(
            ClientRequest::post_json(
                "/api/users",
                &json!({"username": "alice", "password": USER_PASSWORD, "role": "user"}),
            )
            .header("cookie", &admin_cookie),
        )
        .await;
    assert_eq!(created.status, 201, "{}", created.text());

    let user_cookie = login_cookie(&client, "alice", USER_PASSWORD).await;

    // 版本号：两种角色都能读，且只有版本号。
    for cookie in [&admin_cookie, &user_cookie] {
        let response = client
            .send(ClientRequest::new("GET", "/api/version").header("cookie", cookie))
            .await;
        assert_eq!(response.status, 200, "{}", response.text());
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
        assert_eq!(
            body["version"].as_str(),
            Some(env!("CARGO_PKG_VERSION")),
            "版本号应与二进制一致"
        );
        assert_eq!(
            body.as_object().map(|o| o.len()),
            Some(1),
            "只允许返回 version 字段: {body}"
        );
    }

    // 「系统」页的数据仍仅 admin。
    let system_user = client
        .send(ClientRequest::new("GET", "/api/system").header("cookie", &user_cookie))
        .await;
    assert_eq!(system_user.status, 403, "{}", system_user.text());
    let app_info_user = client
        .send(ClientRequest::new("GET", "/api/app/info").header("cookie", &user_cookie))
        .await;
    assert_eq!(app_info_user.status, 403, "{}", app_info_user.text());

    let system_admin = client
        .send(ClientRequest::new("GET", "/api/system").header("cookie", &admin_cookie))
        .await;
    assert_eq!(system_admin.status, 200, "{}", system_admin.text());
}

#[tokio::test]
async fn version_requires_authentication() {
    let (client, _api) = start_api("version-anon").await;
    let anonymous = client
        .send(ClientRequest::new("GET", "/api/version"))
        .await;
    assert_eq!(anonymous.status, 401, "{}", anonymous.text());
}
