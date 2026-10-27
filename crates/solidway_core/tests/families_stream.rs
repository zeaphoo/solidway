//! Anthropic 流式与用量：SSE、别名回写、count 端点。

mod common;

use common::*;
use solidway_core::TokenUsage;

#[tokio::test]
async fn anthropic_non_stream_alias_rewrites_response_model() {
    let upstream = spawn_mock(Behavior::Echo).await;
    let mut cfg = base_config(
        vec![anthropic_provider(
            "p1",
            upstream.base_url(),
            &["claude-alias"],
        )],
        fast_options(),
    );
    cfg.providers[0].models = vec![alias("claude-alias", "claude-sonnet-4-5")];
    let (base, _core) = spawn_core(cfg).await;

    let raw = br#"{"model":"claude-alias","max_tokens":8}"#;
    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        raw.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        upstream.requests()[0].body.as_ref(),
        br#"{"model":"claude-sonnet-4-5","max_tokens":8}"#.as_slice()
    );

    let body = parse_json(resp).await;
    assert_eq!(body["model"], "claude-alias");
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn anthropic_stream_reports_usage() {
    let upstream = spawn_mock(Behavior::AnthropicStream).await;
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    let (base, core) = spawn_core(cfg).await;
    let mut events = core.subscribe();

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude","max_tokens":8,"stream":true}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let _ = resp.text().await.expect("stream body");

    let completion = next_completion(&mut events).await;
    assert!(completion.generation);
    assert_eq!(
        completion.usage,
        Some(TokenUsage {
            input_tokens: 130,
            output_tokens: 50,
            cached_tokens: 20,
            cache_write_tokens: 10,
            reasoning_tokens: 0,
        })
    );
}
