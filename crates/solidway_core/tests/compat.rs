//! 上游兼容适配（表示层归一化 / 自愈重试 / 能力路由）的端到端行为。

mod common;

use common::*;
use serde_json::json;
use solidway_core::{ApiFormat, ModelMapping};

const DASH_SCOPE_ROLE_ERROR: &str = r#"{"error":{"code":"invalid_parameter_error","param":null,"message":"developer is not one of ['system', 'assistant', 'user', 'tool', 'function']","type":"invalid_request_error"}}"#;

fn emulated() -> serde_json::Value {
    json!({"compat": {"developer_role": "emulated"}})
}

fn provider_with_compat(id: &str, base_url: String, compat: serde_json::Value) -> ProviderSpec {
    ProviderSpec {
        options: Some(compat),
        ..provider(id, base_url, &["deepseek-flash"])
    }
}

#[tokio::test]
async fn developer_role_is_normalized_for_strict_upstream() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider_with_compat("p1", upstream.base_url(), emulated())],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "messages": [
            {"role": "developer", "content": "be terse"},
            {"role": "user", "content": "hi"},
        ],
        "max_tokens": 8,
    });
    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        body.clone(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let recorded = upstream.requests();
    assert_eq!(recorded.len(), 1);
    let expected = serde_json::to_string(&body)
        .unwrap()
        .replace(r#""role":"developer""#, r#""role":"system""#);
    // 除角色名外逐字节一致（键序、字段值都不动）。
    assert_eq!(recorded[0].body.as_ref(), expected.as_bytes());
}

#[tokio::test]
async fn native_profile_keeps_bytes_identical() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["deepseek-flash"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "messages": [{"role": "developer", "content": "be terse"}],
        "store": false,
    });
    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        body.clone(),
    )
    .await;
    assert_eq!(resp.status(), 200);

    let recorded = upstream.requests();
    assert_eq!(
        recorded[0].body.as_ref(),
        serde_json::to_vec(&body).unwrap().as_slice(),
        "native 档必须逐字节透传"
    );
}

#[tokio::test]
async fn known_signature_is_rewritten_and_retried_once() {
    let upstream = spawn_mock(Behavior::FailTimes {
        status: 400,
        body: DASH_SCOPE_ROLE_ERROR.to_string(),
        times: 1,
    })
    .await;
    // provider 未声明适配档：完全依赖自愈签名。
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["deepseek-flash"])],
        fast_options(),
    );
    let (base, core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "messages": [{"role": "developer", "content": "be terse"}, {"role": "user", "content": "hi"}],
    });
    let resp = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        body.clone(),
    )
    .await;
    assert_eq!(resp.status(), 200, "命中签名后应改写并重试成功");

    let recorded = upstream.requests();
    assert_eq!(recorded.len(), 2, "同 provider 只重试一次");
    assert!(String::from_utf8_lossy(&recorded[0].body).contains(r#""role":"developer""#));
    assert!(String::from_utf8_lossy(&recorded[1].body).contains(r#""role":"system""#));
    assert_eq!(core.compat_rewrites("p1"), 1);
}

#[tokio::test]
async fn learned_rule_is_applied_preflight_on_the_next_request() {
    let upstream = spawn_mock(Behavior::FailTimes {
        status: 400,
        body: DASH_SCOPE_ROLE_ERROR.to_string(),
        times: 1,
    })
    .await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["deepseek-flash"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "messages": [{"role": "developer", "content": "be terse"}],
    });
    let first = post(
        &base,
        "/v1/chat/completions",
        Some(CLIENT_KEY),
        body.clone(),
    )
    .await;
    assert_eq!(first.status(), 200);
    let hits_after_first = upstream.hit_count();
    assert_eq!(hits_after_first, 2);

    let second = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body).await;
    assert_eq!(second.status(), 200);
    let recorded = upstream.requests();
    assert_eq!(
        recorded.len(),
        hits_after_first + 1,
        "第二次请求不应再触发重试"
    );
    assert!(
        String::from_utf8_lossy(&recorded.last().unwrap().body).contains(r#""role":"system""#),
        "学习到的规则应在预检阶段生效"
    );
}

#[tokio::test]
async fn image_request_skips_provider_that_declares_no_vision() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let spec = ProviderSpec {
        options: Some(json!({"compat": {"images": "unsupported"}})),
        ..provider("p1", upstream.base_url(), &["deepseek-flash"])
    };
    let cfg = base_config(vec![spec], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "what is this"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
            ],
        }],
    });
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body).await;
    assert_eq!(resp.status(), 503, "没有能承载图片的 provider 时应立即失败");
    assert_eq!(upstream.hit_count(), 0);

    // 纯文本请求仍然走该 provider。
    let text_only = json!({
        "model": "deepseek-flash",
        "messages": [{"role": "user", "content": "hi"}],
    });
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), text_only).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(upstream.hit_count(), 1);
}

#[tokio::test]
async fn models_endpoint_advertises_intersected_compatibility() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let strict = ProviderSpec {
        id: "p1".to_string(),
        options: Some(json!({"compat": {"developer_role": "emulated", "thinking": "qwen"}})),
        ..provider("p1", upstream.base_url(), &["deepseek-flash"])
    };
    let lenient = provider("p2", upstream.base_url(), &["deepseek-flash"]);
    let cfg = base_config(vec![strict, lenient], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let resp = get(&base, "/v1/models", Some(CLIENT_KEY)).await;
    assert_eq!(resp.status(), 200);
    let body = parse_json(resp).await;
    let model = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "deepseek-flash")
        .expect("model entry");
    assert_eq!(model["compatibility"]["developer_role"], "emulated");
    assert_eq!(model["compatibility"]["thinking"], "qwen");
    assert_eq!(model["compatibility"]["store"], "native");
}

#[tokio::test]
async fn anthropic_family_is_untouched_by_chat_compat() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let spec = ProviderSpec {
        options: Some(json!({"compat": {"developer_role": "emulated", "thinking": "qwen"}})),
        api_formats: vec![ApiFormat::AnthropicMessages],
        auth_scheme: "x-api-key".to_string(),
        ..provider("p1", upstream.base_url(), &["deepseek-flash"])
    };
    let cfg = base_config(vec![spec], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "hi"}],
    });
    let resp = post(&base, "/v1/messages", Some(CLIENT_KEY), body.clone()).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        upstream.requests()[0].body.as_ref(),
        serde_json::to_vec(&body).unwrap().as_slice()
    );
}

#[tokio::test]
async fn signature_retry_is_chat_only() {
    let upstream = spawn_mock(Behavior::FailTimes {
        status: 400,
        body: DASH_SCOPE_ROLE_ERROR.to_string(),
        times: 1,
    })
    .await;
    let spec = ProviderSpec {
        api_formats: vec![ApiFormat::AnthropicMessages],
        auth_scheme: "x-api-key".to_string(),
        ..provider("p1", upstream.base_url(), &["deepseek-flash"])
    };
    let cfg = base_config(vec![spec], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "hi"}],
    });
    let resp = post(&base, "/v1/messages", Some(CLIENT_KEY), body).await;
    assert_eq!(resp.status(), 400, "非 chat 家族不参与兼容自愈");
    assert_eq!(upstream.hit_count(), 1, "不得重试");
}

#[tokio::test]
async fn stream_options_drop_is_not_undone_by_the_gateway() {
    let upstream = spawn_mock(Behavior::Sse).await;
    let spec = ProviderSpec {
        options: Some(json!({"compat": {"stream_options": "drop"}})),
        ..provider("p1", upstream.base_url(), &["deepseek-flash"])
    };
    let cfg = base_config(vec![spec], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "stream": true,
        "messages": [{"role": "user", "content": "hi"}],
    });
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body).await;
    assert_eq!(resp.status(), 200);
    let sent = String::from_utf8_lossy(&upstream.requests()[0].body).to_string();
    assert!(!sent.contains("stream_options"), "适配档 drop 后不得再补: {sent}");
}

fn model_with_limits(
    name: &str,
    context_window: Option<u64>,
    max_output_tokens: Option<u64>,
    input: Option<&[&str]>,
) -> ModelMapping {
    ModelMapping {
        name: name.to_string(),
        context_window,
        max_output_tokens,
        input: input.map(|list| list.iter().map(|value| value.to_string()).collect()),
        ..ModelMapping::default()
    }
}

#[tokio::test]
async fn models_endpoint_reports_model_limits_conservatively() {
    let upstream = spawn_mock(Behavior::Ok).await;
    // 同一模型由两个 provider 承载：一份 1M 上下文、一份 128K；模态一份含图片、一份纯文本。
    let big = ProviderSpec {
        models_meta: Some(vec![model_with_limits(
            "deepseek-flash",
            Some(1_000_000),
            Some(384_000),
            Some(&["text", "image"]),
        )]),
        ..provider("p1", upstream.base_url(), &["deepseek-flash"])
    };
    let small = ProviderSpec {
        models_meta: Some(vec![model_with_limits(
            "deepseek-flash",
            Some(128_000),
            Some(16_384),
            Some(&["text"]),
        )]),
        ..provider("p2", upstream.base_url(), &["deepseek-flash"])
    };
    let cfg = base_config(vec![big, small], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let body = parse_json(get(&base, "/v1/models", Some(CLIENT_KEY)).await).await;
    let model = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == "deepseek-flash")
        .expect("model entry");
    // 数值取最小值、模态取交集。
    assert_eq!(model["context_length"], 128_000);
    assert_eq!(model["context_window"], 128_000);
    assert_eq!(model["max_output_tokens"], 16_384);
    assert_eq!(model["top_provider"]["context_length"], 128_000);
    assert_eq!(model["top_provider"]["max_completion_tokens"], 16_384);
    assert_eq!(model["input_modalities"], json!(["text"]));
    assert_eq!(
        model["architecture"]["input_modalities"],
        json!(["text"])
    );
    assert_eq!(model["architecture"]["output_modalities"], json!(["text"]));
}

#[tokio::test]
async fn models_endpoint_omits_limits_when_unknown() {
    let upstream = spawn_mock(Behavior::Ok).await;
    let cfg = base_config(
        vec![provider("p1", upstream.base_url(), &["deepseek-flash"])],
        fast_options(),
    );
    let (base, _core) = spawn_core(cfg).await;

    let body = parse_json(get(&base, "/v1/models", Some(CLIENT_KEY)).await).await;
    let model = &body["data"][0];
    assert!(model.get("context_length").is_none());
    assert!(model.get("max_output_tokens").is_none());
    assert!(model.get("top_provider").is_none());
    assert!(model.get("architecture").is_none());
}

#[tokio::test]
async fn image_request_routes_to_provider_that_declares_vision() {
    let text_only = spawn_mock(Behavior::Ok).await;
    let vision = spawn_mock(Behavior::Ok).await;
    let first = ProviderSpec {
        models_meta: Some(vec![model_with_limits(
            "deepseek-flash",
            Some(1_000_000),
            Some(384_000),
            Some(&["text"]),
        )]),
        ..provider("p1", text_only.base_url(), &["deepseek-flash"])
    };
    let second = ProviderSpec {
        models_meta: Some(vec![model_with_limits(
            "deepseek-flash",
            Some(1_000_000),
            Some(384_000),
            Some(&["text", "image"]),
        )]),
        ..provider("p2", vision.base_url(), &["deepseek-flash"])
    };
    let cfg = base_config(vec![first, second], fast_options());
    let (base, _core) = spawn_core(cfg).await;

    let body = json!({
        "model": "deepseek-flash",
        "messages": [{
            "role": "user",
            "content": [
                {"type": "text", "text": "what is this"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
            ],
        }],
    });
    let resp = post(&base, "/v1/chat/completions", Some(CLIENT_KEY), body).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(text_only.hit_count(), 0, "纯文本 provider 不该收到图片请求");
    assert_eq!(vision.hit_count(), 1);
}
