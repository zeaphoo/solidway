//! 桌面偏好：只读视图、「请求壳应用」的受理语义与模式门禁。

mod common;

use common::{ClientRequest, Harness, MockResponse, SeedRoute};
use serde_json::Value;
use solidway_api::RunMode;

const GATEWAY_KEY: &str = "gw-tenant";

async fn harness(tag: &str, mode: RunMode) -> Harness {
    let harness = common::build_harness_with_mode(tag, "", "/v1", mode, |mocks| {
        vec![
            SeedRoute::new("r-main", "main")
                .provider("p-a", mocks.a.base_url(), &["gpt-5"])
                .key("k-main", GATEWAY_KEY),
        ]
    })
    .await;
    harness.mocks.a.set_fallback(MockResponse::ok("gpt-5"));
    harness
}

fn json(response: &common::ClientResponse) -> Value {
    serde_json::from_str(&response.text()).expect("响应应为 JSON")
}

fn put(path: &str, cookie: &str, body: Value) -> ClientRequest {
    ClientRequest::new("PUT", path)
        .header("cookie", cookie)
        .header("content-type", "application/json")
        .raw_body(serde_json::to_vec(&body).expect("serialize body"))
}

#[tokio::test]
async fn desktop_mode_exposes_prefs_and_status() {
    let harness = harness("desktop-prefs", RunMode::Desktop).await;
    std::fs::write(
        harness.data_root.join("desktop.toml"),
        "schema = 1\nautostart = false\nclose_window_to_tray = true\n",
    )
    .expect("write prefs");
    std::fs::write(
        harness.data_root.join("desktop.status.json"),
        r#"{"apply_id":"3f2a1c9d","ok":true,"error":null,
            "applied":{"autostart":false,"close_window_to_tray":true},"at":1761700000}"#,
    )
    .expect("write status");

    let cookie = harness.admin_cookie().await;
    let response = harness
        .client
        .send(ClientRequest::new("GET", "/api/app/desktop").header("cookie", &cookie))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = json(&response);
    assert_eq!(body["available"], true);
    assert_eq!(body["prefs_source"], "file");
    assert_eq!(body["prefs"]["autostart"], false);
    assert_eq!(body["prefs"]["close_window_to_tray"], true);
    assert_eq!(body["status"]["apply_id"], "3f2a1c9d");
    assert_eq!(body["status"]["ok"], true);
}

#[tokio::test]
async fn desktop_mode_accepts_put_and_issues_apply_id() {
    let harness = harness("desktop-put", RunMode::Desktop).await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(put(
            "/api/app/desktop",
            &cookie,
            serde_json::json!({ "autostart": true, "close_window_to_tray": false }),
        ))
        .await;
    assert_eq!(response.status, 202, "{}", response.text());
    let body = json(&response);
    let apply_id = body["apply_id"].as_str().expect("apply_id");
    assert_eq!(apply_id.len(), 8, "apply_id 应为 8 位 hex：{apply_id}");
    assert!(apply_id.chars().all(|c| c.is_ascii_hexdigit()));
    assert_eq!(body["pending"], true);

    // 服务端只发事件、不写文件：壳落盘前两个文件都不存在。
    assert!(!harness.data_root.join("desktop.toml").exists());
    assert!(!harness.data_root.join("desktop.status.json").exists());
}

#[tokio::test]
async fn desktop_mode_rejects_invalid_body() {
    let harness = harness("desktop-invalid", RunMode::Desktop).await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(put(
            "/api/app/desktop",
            &cookie,
            serde_json::json!({ "autostart": "yes" }),
        ))
        .await;
    assert_eq!(response.status, 422, "{}", response.text());
}

#[tokio::test]
async fn standalone_mode_marks_unavailable_and_rejects_put() {
    let harness = harness("desktop-standalone", RunMode::Standalone).await;
    let cookie = harness.admin_cookie().await;

    let response = harness
        .client
        .send(ClientRequest::new("GET", "/api/app/desktop").header("cookie", &cookie))
        .await;
    assert_eq!(response.status, 200, "{}", response.text());
    let body = json(&response);
    assert_eq!(body["available"], false);
    assert!(body["prefs"].is_null());
    assert!(body["status"].is_null());

    let response = harness
        .client
        .send(put(
            "/api/app/desktop",
            &cookie,
            serde_json::json!({ "autostart": true, "close_window_to_tray": true }),
        ))
        .await;
    assert_eq!(response.status, 409, "{}", response.text());
    assert_eq!(json(&response)["error"]["code"], "desktop_unsupported");
}
