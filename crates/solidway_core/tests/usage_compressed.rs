//! 压缩响应（`content-encoding`）下的用量提取：旁路解码取 usage，转发字节不变。
//!
//! 对应问题：上游按客户端的 `accept-encoding` 压缩响应，网关不解压就直接解析，
//! 导致 token 统计恒为 0。这里锁定两条契约：
//! 1. 客户端拿到的仍是原始压缩字节（含 `content-encoding` 头）；
//! 2. 完成事件能拿到解码后的 usage。

mod common;

use common::*;
use solidway_core::TokenUsage;

#[tokio::test]
async fn gzip_json_usage_is_observed_without_touching_bytes() {
    let upstream = spawn_mock(Behavior::GzipJson).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["m"])],
        fast_options(),
    );
    let (base, core) = spawn_core(cfg).await;
    let mut events = core.subscribe();

    let response = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "m"}),
    )
    .await;
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("content-encoding")
            .and_then(|value| value.to_str().ok()),
        Some("gzip")
    );
    let body = response.bytes().await.expect("read body");
    // 转发字节与上游一致：仍是同一份 gzip 字节，未被解压重写。
    assert_eq!(
        body.as_ref(),
        gzip_bytes(GZIP_JSON_RAW.as_bytes()).as_slice()
    );

    let completion = next_completion(&mut events).await;
    assert_eq!(
        completion.usage,
        Some(TokenUsage {
            input_tokens: 40,
            output_tokens: 8,
            cached_tokens: 6,
            ..Default::default()
        })
    );
}

#[tokio::test]
async fn gzip_sse_usage_is_observed_without_touching_bytes() {
    let upstream = spawn_mock(Behavior::GzipSse).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["m"])],
        fast_options(),
    );
    let (base, core) = spawn_core(cfg).await;
    let mut events = core.subscribe();

    let response = post_raw(
        &base,
        "/v1/chat/completions",
        &[("authorization", &format!("Bearer {CLIENT_KEY}"))],
        br#"{"model":"m","stream":true}"#.to_vec(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let body = response.bytes().await.expect("read body");
    assert_eq!(
        body.as_ref(),
        gzip_bytes(GZIP_SSE_RAW.as_bytes()).as_slice()
    );

    let completion = next_completion(&mut events).await;
    assert_eq!(
        completion.usage,
        Some(TokenUsage {
            input_tokens: 100,
            output_tokens: 20,
            ..Default::default()
        })
    );
}

#[tokio::test]
async fn streaming_chat_gets_include_usage_injected() {
    let upstream = spawn_mock(Behavior::Sse).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["m"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let raw = br#"{"model":"m","stream":true}"#;
    let response = post_raw(
        &base,
        "/v1/chat/completions",
        &[("authorization", &format!("Bearer {CLIENT_KEY}"))],
        raw.to_vec(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let _ = response.bytes().await.expect("read body");

    // 上游收到的是「原文 + 顶层 stream_options.include_usage」。
    assert_eq!(
        upstream.requests()[0].body.as_ref(),
        br#"{"model":"m","stream":true,"stream_options":{"include_usage":true}}"#.as_slice()
    );
}

#[tokio::test]
async fn non_streaming_and_explicit_stream_options_are_untouched() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["m"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let plain = br#"{"model":"m"}"#;
    let response = post_raw(
        &base,
        "/v1/chat/completions",
        &[("authorization", &format!("Bearer {CLIENT_KEY}"))],
        plain.to_vec(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let _ = response.bytes().await.expect("read body");

    let explicit = br#"{"model":"m","stream":true,"stream_options":{"include_usage":false}}"#;
    let response = post_raw(
        &base,
        "/v1/chat/completions",
        &[("authorization", &format!("Bearer {CLIENT_KEY}"))],
        explicit.to_vec(),
    )
    .await;
    assert_eq!(response.status(), 200);
    let _ = response.bytes().await.expect("read body");

    let requests = upstream.requests();
    assert_eq!(requests[0].body.as_ref(), plain.as_slice());
    assert_eq!(requests[1].body.as_ref(), explicit.as_slice());
}
