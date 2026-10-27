//! Anthropic Messages 入站通路的集成测试。
//!
//! 覆盖：家族根剥离与上游地址、入站鉴权头、query 清理、Model 来源（body）、
//! 别名回写位置、非生成端点的用量语义、自造错误体家族外壳、跨 provider 换号。

mod common;

use std::time::Duration;

use common::*;
use solidway_core::{ApiFormat, CoreEvent, CoreOptions, hash_api_key};

fn fast_options() -> CoreOptions {
    CoreOptions {
        connect_timeout: Duration::from_secs(5),
        first_byte_timeout: Duration::from_secs(5),
        ..CoreOptions::default()
    }
}

fn anthropic_provider(id: &str, base_url: String, models: &[&str]) -> ProviderSpec {
    provider_with(
        id,
        base_url,
        models,
        vec![ApiFormat::AnthropicMessages],
        "x-api-key",
    )
}

#[tokio::test]
async fn anthropic_forwarding_is_faithful_and_replaces_auth() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![anthropic_provider(
            "p1",
            upstream.base_url(),
            &["claude-alias"],
        )],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let raw = br#"{"model":"claude-alias","max_tokens":8,"metadata":{"user_id":"u1"},"thinking":{"type":"enabled"},"x_custom":1}"#;
    let resp = post_raw(
        &base,
        "/v1/messages",
        &[
            ("x-api-key", CLIENT_KEY),
            ("anthropic-version", "2023-06-01"),
            ("anthropic-beta", "beta-x"),
        ],
        raw.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let recorded = upstream.requests();
    assert_eq!(recorded.len(), 1);
    // 家族根只剥离一次：base_url 已含 /v1，目标为 /v1/messages。
    assert_eq!(recorded[0].uri, "/v1/messages");
    // 出站鉴权头按 provider 的 auth_scheme 重建，客户端凭据不外传。
    assert_eq!(recorded[0].headers.get("x-api-key").unwrap(), "sk-p1");
    assert!(recorded[0].headers.get("authorization").is_none());
    // 家族头原样转发。
    assert_eq!(
        recorded[0].headers.get("anthropic-version").unwrap(),
        "2023-06-01"
    );
    assert_eq!(recorded[0].headers.get("anthropic-beta").unwrap(), "beta-x");
    // 保真：body 逐字节一致。
    assert_eq!(recorded[0].body.as_ref(), raw.as_slice());
}

#[tokio::test]
async fn anthropic_invalid_key_returns_anthropic_error_body() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", "wrong")],
        br#"{"model":"claude"}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 401);
    let body = parse_json(resp).await;
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], "authentication_error");
    assert_eq!(body["error"]["code"], "invalid_api_key");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn anthropic_model_not_in_route_is_404_in_family_shape() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"not-there","max_tokens":8}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = parse_json(resp).await;
    assert_eq!(body["error"]["type"], "not_found_error");
    assert_eq!(body["error"]["code"], "model_not_found");
}

#[tokio::test]
async fn anthropic_without_format_support_is_404() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let mut cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    cfg.providers[0].api_formats = vec![ApiFormat::ChatCompletions];
    cfg.providers[0].auth_scheme = "x-api-key".to_string();
    let (base, _core) = spawn_core(cfg).await;

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude","max_tokens":8}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "unsupported_api_format");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn anthropic_stream_rewrites_message_model_only() {
    let upstream = spawn_mock(Behavior::AnthropicStream).await;
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

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude-alias","max_tokens":8,"stream":true}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    let text = resp.text().await.unwrap();
    // 请求体已把别名换成上游 id。
    assert_eq!(
        upstream.requests()[0].body.as_ref(),
        br#"{"model":"claude-sonnet-4-5","max_tokens":8,"stream":true}"#.as_slice()
    );
    // 流式回写点在 message.model。
    assert!(text.contains(r#""model":"claude-alias""#), "{text}");
    assert!(!text.contains("claude-sonnet-4-5"), "{text}");
    // 其他事件与 [DONE] 原样保留。
    assert!(text.contains(r#"data: {"type":"message_delta""#), "{text}");
    assert!(text.contains("data: [DONE]"), "{text}");
}

#[tokio::test]
async fn count_tokens_is_forwarded_but_not_counted_as_usage() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    let (base, core) = spawn_core(cfg).await;
    let mut events = core.subscribe();

    let resp = post_raw(
        &base,
        "/v1/messages/count_tokens",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude","messages":[{"role":"user","content":"hi"}]}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(upstream.requests()[0].uri, "/v1/messages/count_tokens");

    let mut completion = None;
    while let Ok(event) = events.try_recv() {
        if let CoreEvent::Completion(event) = event {
            completion = Some(event);
        }
    }
    let completion = completion.expect("completion event");
    assert!(!completion.generation, "count_tokens 是非生成端点");
    assert!(completion.usage.is_none());
}

#[tokio::test]
async fn anthropic_failover_between_providers_is_transparent() {
    let limited = spawn_mock(Behavior::Status(429, "{}".to_string())).await;
    let healthy = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            anthropic_provider("p1", limited.base_url(), &["claude"]),
            anthropic_provider("p2", healthy.base_url(), &["claude"]),
        ],
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
    assert_eq!(resp.status(), 200, "换号对客户端透明");
    assert_eq!(limited.hit_count(), 1);
    assert_eq!(healthy.hit_count(), 1);
    assert_eq!(
        healthy.requests()[0].headers.get("x-api-key").unwrap(),
        "sk-p2"
    );
}

#[tokio::test]
async fn models_endpoint_stays_local_for_anthropic_routes() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = get(&base, "/v1/models", Some(CLIENT_KEY)).await;
    assert_eq!(resp.status(), 200);
    let body = parse_json(resp).await;
    assert_eq!(body["object"], "list");
    assert_eq!(body["data"][0]["id"], "claude");
    // OpenAI list 字段 + Anthropic SDK 可识别的扩展字段。
    assert_eq!(body["data"][0]["object"], "model");
    assert_eq!(body["data"][0]["created"], 0);
    assert_eq!(body["data"][0]["owned_by"], "main");
    assert_eq!(body["data"][0]["type"], "model");
    assert_eq!(body["data"][0]["display_name"], "claude");
    assert_eq!(body["data"][0]["created_at"], "1970-01-01T00:00:00Z");
    assert_eq!(body["has_more"], false);
    assert_eq!(body["first_id"], "claude");
    assert_eq!(body["last_id"], "claude");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn all_two_inbound_auth_headers_work() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    for header in ["authorization", "x-api-key"] {
        let value = if header == "authorization" {
            format!("Bearer {CLIENT_KEY}")
        } else {
            CLIENT_KEY.to_string()
        };
        let resp = post_raw(
            &base,
            "/v1/chat/completions",
            &[(header, value.as_str())],
            br#"{"model":"gpt-5","messages":[]}"#.to_vec(),
        )
        .await;
        assert_eq!(resp.status(), 200, "入站头 {header} 应可用");
    }
    assert_eq!(upstream.hit_count(), 2);
}

#[tokio::test]
async fn x_goog_api_key_is_not_accepted() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post_raw(
        &base,
        "/v1/chat/completions",
        &[("x-goog-api-key", CLIENT_KEY)],
        br#"{"model":"gpt-5","messages":[]}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 401);
    let body = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "invalid_api_key");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn api_key_expiry_applies_to_every_inbound_header() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let mut cfg = base_config(
        vec![anthropic_provider("p1", upstream.base_url(), &["claude"])],
        fast_options(),
    );
    cfg.api_keys[0].expires_at = Some(1);
    cfg.api_keys[0].key_hash = hash_api_key(CLIENT_KEY);
    let (base, _core) = spawn_core(cfg).await;

    let resp = post_raw(
        &base,
        "/v1/messages",
        &[("x-api-key", CLIENT_KEY)],
        br#"{"model":"claude","max_tokens":8}"#.to_vec(),
    )
    .await;
    assert_eq!(resp.status(), 401);
    assert_eq!(upstream.hit_count(), 0);
}
