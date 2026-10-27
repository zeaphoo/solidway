//! Anthropic 的负路径：家族错误外壳、未建模路径、413、耗尽与上游错误透传。

mod common;

use common::*;

#[tokio::test]
async fn anthropic_route_exhausted_returns_503_with_anthropic_shell() {
    let upstream = spawn_mock(Behavior::Status(401, "{}".to_string())).await;
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let first = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude","max_tokens":8}"#.to_vec(),
    )
    .await;
    assert_eq!(first.status(), 401);

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude","max_tokens":8}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 503);
    let body = parse_json(resp).await;
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "api_error");
    assert_eq!(body["error"]["code"], "route_exhausted");
}

#[tokio::test]
async fn anthropic_body_over_limit_is_413_with_anthropic_shell() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let options = solidway_core::CoreOptions {
        request_body_limit: 32,
        ..fast_options()
    };
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        options,
    );
    let (base, _core) = spawn_core(cfg).await;

    let big = "x".repeat(200);
    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        format!(r#"{{"model":"claude","pad":"{big}"}}"#).into_bytes(),
    )
    .await;
    assert_eq!(resp.status(), 413);
    let body = parse_json(resp).await;
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "request_too_large");
    assert_eq!(body["error"]["code"], "request_too_large");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn unmodelled_v1beta_paths_use_openai_shell() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    // 已鉴权：未建模路径 → OpenAI 外壳的 unsupported_endpoint。
    let resp = post_raw(
        &base,
        "/v1beta/batches",
        &[("authorization", &format!("Bearer {CLIENT_KEY}"))],
        br#"{}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "unsupported_endpoint");
    assert!(body["error"]["status"].is_null());

    // 未鉴权也必须按 path 家族成形：这里同样回退 OpenAI 外壳。
    let resp = post_raw(&base, "/v1beta/batches", &[], br#"{}"#.to_vec()).await;
    assert_eq!(resp.status(), 401);
    let body = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "invalid_api_key");
    assert!(body["error"]["status"].is_null());

    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn anthropic_upstream_error_is_passed_through() {
    let raw =
        r#"{"type":"error","error":{"type":"invalid_request_error","message":"bad request"}}"#;
    let upstream = spawn_mock(Behavior::Status(400, raw.to_string())).await;
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude","max_tokens":8}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 400);
    assert_eq!(resp.text().await.unwrap(), raw);
}

/// query 里的凭据不参与鉴权：只有头才是凭据来源。
#[tokio::test]
async fn query_credential_does_not_authenticate() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    for path in [
        format!("/v1/chat/completions?key={CLIENT_KEY}"),
        format!("/v1/chat/completions?api_key={CLIENT_KEY}"),
    ] {
        let resp = post_raw(
            &base,
            &path,
            &[],
            br#"{"model":"gpt-5","messages":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(resp.status(), 401, "{path}");
        let body = parse_json(resp).await;
        assert_eq!(body["error"]["code"], "invalid_api_key", "{path}");
    }
    assert_eq!(upstream.hit_count(), 0, "query 凭据不得触达上游");
}
