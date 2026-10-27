//! admin 引导（一次性令牌）与密码重置的端到端测试。
//!
//! 覆盖 `openspec/changes/2026-10-18-bootstrap-admin-login` 的验收点：
//! 锁定态不可登录、令牌一次性、口令过短被拒、设置完成即获会话、重置后可重新引导、
//! 以及「明文令牌只存在于签发方内存」这一不变量。

mod common;

use common::{ClientRequest, RawClient};
use serde_json::json;
use solidway_api::auth::password;
use solidway_api::{Api, ApiConfig, RunMode};

/// 启动一个仅管理面（不含数据面）的实例，返回地址与 API 句柄。
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

    let app = api.router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (RawClient::new(&format!("http://{addr}")), api)
}

fn pending(client: &RawClient) -> impl std::future::Future<Output = bool> + '_ {
    async move {
        let response = client
            .send(ClientRequest::new("GET", "/api/auth/bootstrap"))
            .await;
        assert_eq!(response.status, 200, "pending 查询应成功");
        let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
        assert!(
            body.get("token").is_none() && body.get("token_hash").is_none(),
            "pending 响应不得泄露令牌: {body}"
        );
        body["pending"].as_bool().expect("pending bool")
    }
}

async fn login(client: &RawClient, password: &str) -> u16 {
    client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({"username": "admin", "password": password}),
        ))
        .await
        .status
}

#[tokio::test]
async fn unknown_user_login_runs_password_verification() {
    // R3-F-6：用户名不存在时也要跑一次等价 Argon2 校验，避免用户名枚举时序。
    let (client, _api) = start_api("login-timing").await;
    let started = std::time::Instant::now();
    let response = client
        .send(ClientRequest::post_json(
            "/api/auth/login",
            &json!({"username": "nobody", "password": "whatever-pass"}),
        ))
        .await;
    let elapsed = started.elapsed();
    assert_eq!(response.status, 401);
    assert!(
        elapsed >= std::time::Duration::from_millis(10),
        "未知用户登录必须执行口令校验（R3-F-6），实际 {elapsed:?}"
    );
}

#[tokio::test]
async fn first_run_locks_admin_and_issues_one_token() {
    let (client, api) = start_api("bootstrap-lock").await;

    let token = api.bootstrap_token().expect("首启应签发引导令牌").to_string();
    assert!(!token.is_empty());

    // 真正需要持久化的只有「锁定态」：admin 的口令哈希不可用。
    let hash: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE username = 'admin'")
            .fetch_one(&api.state().pool)
            .await
            .expect("read admin hash");
    assert_eq!(hash, solidway_api::auth::password::LOCKED_HASH);

    // 令牌只活在进程内存：库里（整个 db 文件）不得出现明文。
    let data_root = api.state().data_root.clone();
    let db_bytes = std::fs::read(data_root.join("solidway.db")).expect("read db file");
    let needle = token.as_bytes();
    assert!(
        !db_bytes.windows(needle.len()).any(|window| window == needle),
        "引导令牌明文不得落库"
    );

    // 锁定态：任何口令都登不进去。
    assert_eq!(login(&client, "").await, 401);
    assert_eq!(login(&client, &token).await, 401);

    assert!(pending(&client).await, "首启应处于待引导状态");
}

#[tokio::test]
async fn bootstrap_sets_password_and_logs_in() {
    let (client, api) = start_api("bootstrap-ok").await;
    let token = api.bootstrap_token().expect("token").to_string();

    let response = client
        .send(ClientRequest::post_json(
            "/api/auth/bootstrap",
            &json!({"token": token, "new_password": "setup-pass-123"}),
        ))
        .await;
    assert_eq!(response.status, 200, "引导应成功: {}", response.text());
    let cookie = response.header("set-cookie").expect("应下发会话 Cookie");
    assert!(cookie.contains("solidway_session="), "{cookie}");
    assert!(cookie.to_ascii_lowercase().contains("httponly"), "{cookie}");
    let body: serde_json::Value = serde_json::from_slice(&response.body).expect("json");
    assert_eq!(body["username"], "admin");

    // 令牌一次性：再次使用被拒。
    let replay = client
        .send(ClientRequest::post_json(
            "/api/auth/bootstrap",
            &json!({"token": api.bootstrap_token().unwrap_or_default(), "new_password": "another-pass-1"}),
        ))
        .await;
    assert_eq!(replay.status, 401);
    assert!(replay.text().contains("invalid_bootstrap"), "{}", replay.text());

    assert!(!pending(&client).await, "完成后不应再有待用令牌");

    // 会话可用，且新口令可正常登录。
    let me = client
        .send(ClientRequest::new("GET", "/api/auth/me").header("cookie", cookie))
        .await;
    assert_eq!(me.status, 200);
    assert_eq!(login(&client, "setup-pass-123").await, 200);
}

#[tokio::test]
async fn bootstrap_rejects_short_password_and_bad_token() {
    let (client, api) = start_api("bootstrap-bad").await;
    let token = api.bootstrap_token().expect("token").to_string();

    let short = client
        .send(ClientRequest::post_json(
            "/api/auth/bootstrap",
            &json!({"token": token, "new_password": "1234567"}),
        ))
        .await;
    assert_eq!(short.status, 422, "{}", short.text());
    assert!(short.text().contains("invalid_request"), "{}", short.text());
    assert!(pending(&client).await, "口令不合法时令牌不得被消费");

    let wrong = client
        .send(ClientRequest::post_json(
            "/api/auth/bootstrap",
            &json!({"token": "not-the-token", "new_password": "long-enough-pass"}),
        ))
        .await;
    assert_eq!(wrong.status, 401);
    assert!(wrong.text().contains("invalid_bootstrap"), "{}", wrong.text());
    assert!(pending(&client).await);
}

#[tokio::test]
async fn reset_password_reissues_token_on_next_start() {
    let (client, api) = start_api("bootstrap-reset").await;
    let data_root = api.state().data_root.clone();
    let first = api.bootstrap_token().expect("token").to_string();

    // 完成一次引导，得到可用口令。
    let done = client
        .send(ClientRequest::post_json(
            "/api/auth/bootstrap",
            &json!({"token": first, "new_password": "first-pass-123"}),
        ))
        .await;
    assert_eq!(done.status, 200, "{}", done.text());
    assert_eq!(login(&client, "first-pass-123").await, 200);

    // 重置：置回锁定态、清令牌（不产生明文）。
    solidway_api::reset_admin_password(&data_root)
        .await
        .expect("reset password");
    assert_eq!(login(&client, "first-pass-123").await, 401, "旧口令应失效");

    // 下一次启动重新签发，且能继续完成引导。
    let api = Api::init(ApiConfig {
        data_root,
        master_key: None,
        mode: RunMode::Standalone,
        web_assets: None,
    })
    .await
    .expect("re-init api");
    let second = api.bootstrap_token().expect("重置后重启应重新签发").to_string();

    let app = api.router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let client = RawClient::new(&format!("http://{addr}"));

    let again = client
        .send(ClientRequest::post_json(
            "/api/auth/bootstrap",
            &json!({"token": second, "new_password": "second-pass-1"}),
        ))
        .await;
    assert_eq!(again.status, 200, "{}", again.text());
    assert_eq!(login(&client, "second-pass-1").await, 200);
}

#[tokio::test]
async fn password_gate_rejects_locked_hash() {
    // 锁定态哈希不能被任何口令通过（含空串与哈希本身）。
    assert!(password::is_locked(password::LOCKED_HASH));
    assert!(!password::is_locked(
        &password::hash_password("whatever").expect("hash")
    ));
    assert!(!password::verify_password("", password::LOCKED_HASH));
    assert!(!password::verify_password(password::LOCKED_HASH, password::LOCKED_HASH));
}
