//! 转发主链路集成测试（移植 develop `tests/forward.rs` 的行为语义）。

mod common;

use common::*;
use solidway_core::{ApiFormat, CoreOptions, ModelMapping};
use std::time::{Duration, Instant};

fn fast_options() -> CoreOptions {
    CoreOptions {
        connect_timeout: Duration::from_secs(5),
        first_byte_timeout: Duration::from_secs(5),
        ..CoreOptions::default()
    }
}

#[tokio::test]
async fn forwards_faithfully_and_replaces_auth() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = serde_json::json!({"model": "gpt-5", "messages": [], "custom_beta": {"x": 1}});
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body.clone()).await;
    assert_eq!(resp.status(), 200);

    let recorded = upstream.requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].uri, "/v1/chat/completions");
    // 白名单改写之一：鉴权头替换为 provider 凭据，客户端凭据不外传。
    assert_eq!(
        recorded[0].headers.get("authorization").unwrap(),
        "Bearer sk-p1"
    );
    // 未知字段原样到达（逐字节一致）。
    assert_eq!(
        recorded[0].body.as_ref(),
        serde_json::to_vec(&body).unwrap().as_slice()
    );
}

#[tokio::test]
async fn invalid_key_is_401() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/chat/completions", Some("wrong"), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 401);
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn expired_api_key_is_401() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let mut cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    cfg.api_keys[0].expires_at = Some(1);
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5", "messages": []}),
    )
    .await;
    assert_eq!(resp.status(), 401);
    assert_eq!(upstream.hit_count(), 0, "过期 Key 不得访问上游");
}

#[tokio::test]
async fn future_expiry_is_accepted() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let mut cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    cfg.api_keys[0].expires_at = Some(i64::MAX);
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5", "messages": []}),
    )
    .await;
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn invalid_key_precedes_endpoint_classification() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/whatever", Some("wrong"), serde_json::json!({})).await;
    assert_eq!(resp.status(), 401, "无效凭据优先于端点分类");
}

#[tokio::test]
async fn unknown_endpoint_is_404() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/whatever", Some(CLIENT_KEY), serde_json::json!({})).await;
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "unsupported_endpoint");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn chat_only_provider_is_not_used_for_responses() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let mut cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    cfg.providers[0].api_formats = vec![ApiFormat::ChatCompletions];
    let (base, _core) = spawn_core(cfg).await;

    // chat completions 仍可走。
    let ok = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5", "messages": []}),
    )
    .await;
    assert_eq!(ok.status(), 200);
    assert_eq!(upstream.hit_count(), 1);

    // responses 没有候选：404 + 明确错误码，不访问上游。
    let resp = post(
        &base,
        "/v1/responses",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5", "input": "hi"}),
    )
    .await;
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "unsupported_api_format");
    assert_eq!(upstream.hit_count(), 1, "格式不匹配不得访问上游");
}

#[tokio::test]
async fn model_alias_rewrites_request_and_response() {
    let upstream = spawn_mock(Behavior::Echo).await;
    let mut cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    cfg.providers[0].models = vec![ModelMapping {
        name: "gpt-5".to_string(),
        upstream: "gpt-5-2025-08-07".to_string(),
    }];
    let (base, _core) = spawn_core(cfg).await;

    let body = serde_json::json!({"model": "gpt-5", "messages": [], "custom": {"keep": 1}});
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body).await;
    assert_eq!(resp.status(), 200);

    let recorded = upstream.requests();
    let sent: serde_json::Value = serde_json::from_slice(&recorded[0].body).expect("json");
    assert_eq!(sent["model"], "gpt-5-2025-08-07", "请求体应改写为上游 id");
    assert_eq!(sent["custom"]["keep"], 1, "其余字段应保留");

    let response: serde_json::Value = parse_json(resp).await;
    assert_eq!(response["model"], "gpt-5", "响应应回写为客户端别名");
}

#[tokio::test]
async fn error_body_is_not_alias_rewritten() {
    // R1-F-8：错误响应体（含 LastFailure 透传的上游错误体）逐字节保留，不回写 model。
    let upstream = spawn_mock(Behavior::Status(
        500,
        r#"{"error":{"message":"upstream exploded","model":"gpt-5-2025-08-07"}}"#.to_string(),
    ))
    .await;
    let mut cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    cfg.providers[0].models = vec![ModelMapping {
        name: "gpt-5".to_string(),
        upstream: "gpt-5-2025-08-07".to_string(),
    }];
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5"}),
    )
    .await;
    assert_eq!(resp.status(), 500);
    let text = resp.text().await.expect("error body");
    assert!(
        text.contains("gpt-5-2025-08-07") && !text.contains("\"model\":\"gpt-5\""),
        "错误体不得回写别名：{text}"
    );
}

#[tokio::test]
async fn all_marked_failures_emit_empty_provider_id() {
    // R1-F-2：换号全败且最后一次为标记类 HTTP 失败时，Access.provider_id 为空。
    let a = spawn_mock(Behavior::Status(429, "{}".to_string())).await;
    let b = spawn_mock(Behavior::Status(429, "{}".to_string())).await;
    let cfg = base_config(
        vec![
            provider("a", a.base_url(), &["gpt-5"]),
            provider("b", b.base_url(), &["gpt-5"]),
        ],
        fast_options(),
    );
    let (base, core) = spawn_core(cfg).await;
    let mut events = core.subscribe();

    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5"}),
    )
    .await;
    assert_eq!(resp.status(), 429);

    let access = loop {
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("access event timeout")
            .expect("event channel");
        if let solidway_core::CoreEvent::Access(access) = event {
            break access;
        }
    };
    assert_eq!(access.status, 429);
    assert_eq!(access.provider_id, "", "全败请求不得归因到最后一个尝试者");
}

#[tokio::test]
async fn model_alias_rewrites_streaming_events() {
    let upstream = spawn_mock(Behavior::SseEcho).await;
    let mut cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    cfg.providers[0].models = vec![ModelMapping {
        name: "gpt-5".to_string(),
        upstream: "gpt-5-2025-08-07".to_string(),
    }];
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(
        &base,
        "/v1/responses",
        Some(CLIENT_KEY),
        serde_json::json!({"model": "gpt-5", "input": "hi"}),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let text = resp.text().await.expect("sse text");
    assert!(text.contains("\"model\":\"gpt-5\""), "流式事件应回写别名：{text}");
    assert!(!text.contains("gpt-5-2025-08-07"), "上游 id 不应泄漏：{text}");
    assert!(text.contains("data: [DONE]"), "结尾事件应保留");
}

#[tokio::test]
async fn health_is_local_and_unauthenticated() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = get(&base, "/health", None).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn models_returns_union_and_rejects_non_get() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            provider("p1", upstream.base_url(), &["gpt-5", "gpt-5-mini"]),
            provider("p2", upstream.base_url(), &["gpt-5", "o3"]),
        ],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = get(&base, "/v1/models", Some(CLIENT_KEY)).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = parse_json(resp).await;
    let mut ids: Vec<String> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    assert_eq!(ids, ["gpt-5", "gpt-5-mini", "o3"]);

    let resp = post(&base, "/v1/models", Some(CLIENT_KEY), serde_json::json!({})).await;
    assert_eq!(resp.status(), 405);
    assert_eq!(resp.headers().get("allow").unwrap(), "GET");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn model_not_in_route_is_404() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    // 缺失 model
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"messages": []})).await;
    assert_eq!(resp.status(), 404);
    // 不在清单
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "nope"})).await;
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "model_not_found");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn empty_models_serves_nothing() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(vec![provider("p1", upstream.base_url(), &[])], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 404, "空 models 不服务任何模型");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn query_is_passed_through() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/v1/responses?foo=bar"))
        .header("authorization", format!("Bearer {CLIENT_KEY}"))
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&serde_json::json!({"model": "gpt-5"})).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(upstream.requests()[0].uri, "/v1/responses?foo=bar");
}

#[tokio::test]
async fn failover_to_next_provider_on_429() {
    let bad = spawn_mock(Behavior::Status(429, "{\"error\":\"slow\"}".to_string())).await;
    let good = spawn_mock(Behavior::Ok).await;
    // 轮询从第一条开始 → 先打 bad，换号到 good。
    let cfg = base_config(
        vec![
            provider("bad", bad.base_url(), &["gpt-5"]),
            provider("good", good.base_url(), &["gpt-5"]),
        ],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200, "换号后客户端只看到成功响应");
    assert_eq!(bad.hit_count(), 1);
    assert_eq!(good.hit_count(), 1);
    assert_eq!(good.requests()[0].headers.get("authorization").unwrap(), "Bearer sk-good");
}

#[tokio::test]
async fn all_providers_failing_returns_last_error() {
    let a = spawn_mock(Behavior::Status(429, "{\"from\":\"a\"}".to_string())).await;
    let b = spawn_mock(Behavior::Status(429, "{\"from\":\"b\"}".to_string())).await;
    let cfg = base_config(
        vec![
            provider("a", a.base_url(), &["gpt-5"]),
            provider("b", b.base_url(), &["gpt-5"]),
        ],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 429);
    assert_eq!(resp.text().await.unwrap(), "{\"from\":\"b\"}");
}

#[tokio::test]
async fn request_level_error_is_not_failovered() {
    let a = spawn_mock(Behavior::Status(400, "{\"error\":\"bad request\"}".to_string())).await;
    let b = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![
            provider("a", a.base_url(), &["gpt-5"]),
            provider("b", b.base_url(), &["gpt-5"]),
        ],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 400);
    assert_eq!(b.hit_count(), 0, "请求级错误不换号");
}

#[tokio::test]
async fn exhausted_route_returns_503() {
    let upstream = spawn_mock(Behavior::Status(401, "{}".to_string())).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    // 第一次：401 → 该凭据被冷却。
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 401);
    // 第二次：唯一凭据不可用 → 503。
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 503);
    let body: serde_json::Value = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "route_exhausted");
}

#[tokio::test]
async fn body_over_limit_is_413() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let options = CoreOptions {
        request_body_limit: 32,
        ..fast_options()
    };
    let cfg = base_config(vec![provider("p1", upstream.base_url(), &["gpt-5"])], options);
    let (base, _core) = spawn_core(cfg).await;

    let big = "x".repeat(200);
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5", "pad": big})).await;
    assert_eq!(resp.status(), 413);
    let body: serde_json::Value = parse_json(resp).await;
    assert_eq!(body["error"]["code"], "request_too_large");
    assert_eq!(upstream.hit_count(), 0);
}

#[tokio::test]
async fn slow_error_body_times_out_and_fails_over() {
    // 上游先回 429 头，再挂住错误体；读取超时后应立刻换号到健康上游。
    let slow = spawn_mock(Behavior::SlowBody {
        status: 429,
        body: "{}".to_string(),
        delay: Duration::from_secs(3),
    })
    .await;
    let good = spawn_mock(Behavior::Ok).await;
    let options = CoreOptions {
        response_body_timeout: Duration::from_millis(150),
        ..fast_options()
    };
    let cfg = base_config(
        vec![
            provider("slow", slow.base_url(), &["gpt-5"]),
            provider("good", good.base_url(), &["gpt-5"]),
        ],
        options,
    );
    let (base, _core) = spawn_core(cfg).await;

    let started = Instant::now();
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "错误体挂住不应拖慢换号：{:?}",
        started.elapsed()
    );
    assert_eq!(slow.hit_count(), 1);
    assert_eq!(good.hit_count(), 1);
}

#[tokio::test]
async fn slow_success_body_times_out() {
    let slow = spawn_mock(Behavior::SlowBody {
        status: 200,
        body: r#"{"ok":true}"#.to_string(),
        delay: Duration::from_secs(3),
    })
    .await;
    let options = CoreOptions {
        response_body_timeout: Duration::from_millis(150),
        ..fast_options()
    };
    let cfg = base_config(vec![provider("p1", slow.base_url(), &["gpt-5"])], options);
    let (base, _core) = spawn_core(cfg).await;

    let started = Instant::now();
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "非流式响应体读取必须有超时：{:?}",
        started.elapsed()
    );
    assert!(resp.bytes().await.unwrap().is_empty(), "读取失败应按错误处理");
}

#[tokio::test]
async fn oversized_success_body_is_rejected() {
    let upstream = spawn_mock(Behavior::SlowBody {
        status: 200,
        body: "x".repeat(4096),
        delay: Duration::ZERO,
    })
    .await;
    let options = CoreOptions {
        response_body_limit: 1024,
        ..fast_options()
    };
    let cfg = base_config(vec![provider("p1", upstream.base_url(), &["gpt-5"])], options);
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);
    assert!(
        resp.bytes().await.unwrap().is_empty(),
        "超过响应体上限时应计错而不是静默整读"
    );
}

#[tokio::test]
async fn sse_is_forwarded_through() {
    let upstream = spawn_mock(Behavior::Sse).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["gpt-5"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), serde_json::json!({"model": "gpt-5"})).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/event-stream"
    );
    let text = resp.text().await.unwrap();
    assert!(text.contains("data: {\"a\":1}"));
    assert!(text.contains("data: [DONE]"));
}
