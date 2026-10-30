//! 集成测试脚手架：可脚本化的 mock 上游 + 启动 core 转发服务。
//!
//! 移植自 develop 的 `tests/common`，改为：配置直接以 [`CoreConfig`] 构造（不经 TOML），
//! 通过真实 HTTP 调用 core 与 mock 上游。

#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use solidway_core::{
    ApiFormat, ApiKeyEntry, CompletionEvent, Core, CoreConfig, CoreEvent, CoreOptions,
    ModelMapping, ProviderEntry, RouteEntry, hash_api_key,
};

/// 测试用客户端 API Key 明文。
pub const CLIENT_KEY: &str = "sw-test-key";

/// mock 上游的行为。
#[derive(Clone, Debug)]
pub enum Behavior {
    /// 200 + JSON。
    Ok,
    /// 指定状态码 + 文本体。
    Status(u16, String),
    /// SSE 流。
    Sse,
    /// 回显请求体里的 `model`（测试别名回写）。
    Echo,
    /// 分块 SSE，事件回显请求体里的 `model`（测试流式改写与跨 chunk 行缓冲）。
    SseEcho,
    /// Anthropic Messages 风格 SSE：`message_start.message.model` 回显请求体 `model`。
    AnthropicStream,
    /// 先回响应头，等待 `delay` 后再发 body（用于响应体读取超时 / 上限测试）。
    SlowBody {
        status: u16,
        body: String,
        delay: std::time::Duration,
    },
    /// `content-encoding: gzip` 的 JSON 响应（带 usage），用于旁路解码取用量。
    GzipJson,
    /// `content-encoding: gzip` 的 SSE 响应（带 usage chunk），跨块下发。
    GzipSse,
}

/// gzip 压缩固定的 JSON 响应体（`GzipJson` 行为）。
pub const GZIP_JSON_RAW: &str = r#"{"id":"x","usage":{"prompt_tokens":40,"completion_tokens":8,"prompt_tokens_details":{"cached_tokens":6}}}"#;

/// gzip 压缩固定的 SSE 响应体（`GzipSse` 行为）：第二个 chunk 携带 usage。
pub const GZIP_SSE_RAW: &str = concat!(
    "data: {\"id\":\"x\",\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n",
    "data: {\"id\":\"x\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":20}}\n\n",
    "data: [DONE]\n\n",
);

/// 测试用 gzip 压缩。
pub fn gzip_bytes(raw: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}

/// 上游收到的一次请求（保真断言用）。
#[derive(Clone, Debug)]
pub struct Recorded {
    pub method: String,
    pub uri: String,
    pub headers: HeaderMap,
    pub body: Bytes,
}

#[derive(Clone)]
pub struct Mock {
    addr: SocketAddr,
    requests: Arc<Mutex<Vec<Recorded>>>,
    behavior: Arc<Mutex<Behavior>>,
}

impl Mock {
    /// 默认上游 API 根（`/v1`）。
    pub fn base_url(&self) -> String {
        self.base_url_at("/v1")
    }

    /// 指定上游 API 根（如 `/v1beta`）。
    pub fn base_url_at(&self, root: &str) -> String {
        format!("http://{}{root}", self.addr)
    }

    /// 已收到的请求（按顺序）。
    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }

    pub fn hit_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    pub fn set_behavior(&self, behavior: Behavior) {
        *self.behavior.lock().unwrap() = behavior;
    }
}

async fn mock_handler(
    State((requests, behavior)): State<(Arc<Mutex<Vec<Recorded>>>, Arc<Mutex<Behavior>>)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    requests.lock().unwrap().push(Recorded {
        method: method.to_string(),
        uri: uri.to_string(),
        headers: headers.clone(),
        body: body.clone(),
    });
    match behavior.lock().unwrap().clone() {
        Behavior::Ok => axum::Json(serde_json::json!({"ok": true})).into_response(),
        Behavior::Status(code, body) => (StatusCode::from_u16(code).unwrap(), body).into_response(),
        Behavior::SlowBody {
            status,
            body,
            delay,
        } => {
            let stream = futures_util::stream::once(async move {
                tokio::time::sleep(delay).await;
                Ok::<Bytes, std::io::Error>(Bytes::from(body))
            });
            Response::builder()
                .status(StatusCode::from_u16(status).unwrap())
                .header("content-type", "application/json")
                .body(axum::body::Body::from_stream(stream))
                .unwrap()
        }
        Behavior::Sse => {
            let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
                Ok(Bytes::from_static(b"data: {\"a\":1}\n\n")),
                Ok(Bytes::from_static(b"data: [DONE]\n\n")),
            ];
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from_stream(futures_util::stream::iter(
                    chunks,
                )))
                .unwrap()
        }
        Behavior::Echo => {
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let model = value
                .get("model")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            axum::Json(serde_json::json!({"model": model, "ok": true})).into_response()
        }
        Behavior::GzipJson => {
            let encoded = gzip_bytes(GZIP_JSON_RAW.as_bytes());
            let split = encoded.len() / 2;
            let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
                Ok(Bytes::from(encoded[..split].to_vec())),
                Ok(Bytes::from(encoded[split..].to_vec())),
            ];
            Response::builder()
                .header("content-type", "application/json")
                .header("content-encoding", "gzip")
                .body(axum::body::Body::from_stream(futures_util::stream::iter(
                    chunks,
                )))
                .unwrap()
        }
        Behavior::GzipSse => {
            let encoded = gzip_bytes(GZIP_SSE_RAW.as_bytes());
            // 切成 3 块：gzip 头、载荷中段、尾段，验证跨块解码。
            let step = encoded.len() / 3 + 1;
            let chunks: Vec<Result<Bytes, std::io::Error>> = encoded
                .chunks(step)
                .map(|piece| Ok(Bytes::from(piece.to_vec())))
                .collect();
            Response::builder()
                .header("content-type", "text/event-stream")
                .header("content-encoding", "gzip")
                .body(axum::body::Body::from_stream(futures_util::stream::iter(
                    chunks,
                )))
                .unwrap()
        }
        Behavior::AnthropicStream => {
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let model = value.get("model").and_then(|m| m.as_str()).unwrap_or("");
            let start = format!(
                "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"m1\",\"model\":\"{model}\",\"usage\":{{\"input_tokens\":100,\"cache_read_input_tokens\":20,\"cache_creation_input_tokens\":10}}}}}}\n\n"
            );
            let delta = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":50}}\n\n";
            let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
                Ok(Bytes::from(start)),
                Ok(Bytes::from_static(delta.as_bytes())),
                Ok(Bytes::from_static(b"data: [DONE]\n\n")),
            ];
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from_stream(futures_util::stream::iter(
                    chunks,
                )))
                .unwrap()
        }
        Behavior::SseEcho => {
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let model = value.get("model").and_then(|m| m.as_str()).unwrap_or("");
            let event = format!(
                "data: {{\"model\":\"{model}\",\"choices\":[{{\"delta\":{{\"content\":\"hi\"}}}}]}}\n\n"
            );
            // 故意把一行拆成两个 chunk，验证按行缓冲后仍能改写。
            let split = event.len() / 2;
            let chunks: Vec<Result<Bytes, std::io::Error>> = vec![
                Ok(Bytes::from(event[..split].to_string())),
                Ok(Bytes::from(event[split..].to_string())),
                Ok(Bytes::from_static(b"data: [DONE]\n\n")),
            ];
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(axum::body::Body::from_stream(futures_util::stream::iter(
                    chunks,
                )))
                .unwrap()
        }
    }
}

/// 启动 mock 上游。
pub async fn spawn_mock(behavior: Behavior) -> Mock {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let behavior = Arc::new(Mutex::new(behavior));
    let router = Router::new()
        .fallback(mock_handler)
        .with_state((Arc::clone(&requests), Arc::clone(&behavior)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Mock {
        addr,
        requests,
        behavior,
    }
}

/// 一条 provider 凭据的描述（测试便捷构造）。
pub struct ProviderSpec {
    pub id: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub enabled: bool,
    pub api_formats: Vec<ApiFormat>,
    pub auth_scheme: String,
    pub priority: i64,
}

pub fn provider(id: &str, base_url: String, models: &[&str]) -> ProviderSpec {
    ProviderSpec {
        id: id.to_string(),
        base_url,
        models: models.iter().map(|m| m.to_string()).collect(),
        enabled: true,
        api_formats: vec![ApiFormat::ChatCompletions, ApiFormat::Responses],
        auth_scheme: "bearer".to_string(),
        priority: 1,
    }
}

/// 指定入站格式与鉴权方式的 provider 凭据。
pub fn provider_with(
    id: &str,
    base_url: String,
    models: &[&str],
    api_formats: Vec<ApiFormat>,
    auth_scheme: &str,
) -> ProviderSpec {
    ProviderSpec {
        api_formats,
        auth_scheme: auth_scheme.to_string(),
        ..provider(id, base_url, models)
    }
}

/// 模型名 → 上游模型 id 的别名映射。
pub fn alias(name: &str, upstream: &str) -> ModelMapping {
    ModelMapping {
        name: name.to_string(),
        upstream: upstream.to_string(),
    }
}

/// 快速超时的 CoreOptions（集成测试统一使用）。
pub fn fast_options() -> CoreOptions {
    CoreOptions {
        connect_timeout: Duration::from_secs(5),
        first_byte_timeout: Duration::from_secs(5),
        ..CoreOptions::default()
    }
}

/// Anthropic Messages 入站格式的 provider。
pub fn anthropic_provider(id: &str, base_url: String, models: &[&str]) -> ProviderSpec {
    provider_with(
        id,
        base_url,
        models,
        vec![ApiFormat::AnthropicMessages],
        "x-api-key",
    )
}

/// 等待下一条完成事件（跳过 access / attempt）。
pub async fn next_completion(
    events: &mut tokio::sync::broadcast::Receiver<CoreEvent>,
) -> CompletionEvent {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .expect("completion event timeout")
            .expect("completion event");
        if let CoreEvent::Completion(completion) = event {
            return completion;
        }
    }
}

/// 单路由 + 指定 provider + 一个客户端 API Key。
pub fn base_config(providers: Vec<ProviderSpec>, options: CoreOptions) -> CoreConfig {
    let route_id = "r1".to_string();
    CoreConfig {
        api_keys: vec![ApiKeyEntry {
            id: "k1".to_string(),
            route_id: route_id.clone(),
            key_hash: hash_api_key(CLIENT_KEY),
            enabled: true,
            expires_at: None,
        }],
        routes: vec![RouteEntry {
            id: route_id.clone(),
            name: "main".to_string(),
            enabled: true,
            selection: Default::default(),
        }],
        providers: providers
            .into_iter()
            .map(|spec| ProviderEntry {
                id: spec.id.clone(),
                route_id: route_id.clone(),
                name: None,
                provider: "openai".to_string(),
                base_url: spec.base_url,
                api_key: format!("sk-{}", spec.id),
                credential_kind: "api_key".to_string(),
                auth_scheme: spec.auth_scheme,
                options: None,
                api_formats: spec.api_formats,
                models: spec
                    .models
                    .into_iter()
                    .map(|name| ModelMapping {
                        name,
                        upstream: String::new(),
                    })
                    .collect(),
                enabled: spec.enabled,
                priority: spec.priority,
            })
            .collect(),
        health: vec![],
        metrics: vec![],
        affinity: vec![],
        options,
    }
}

/// 启动 core 转发服务，返回其地址与句柄。
pub async fn spawn_core(config: CoreConfig) -> (String, Arc<Core>) {
    let core = Core::new(config).await.expect("build core");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = core.router();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}"), core)
}

/// 构造带客户端凭据的 POST 请求。
pub async fn post_raw(
    base: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Vec<u8>,
) -> reqwest::Response {
    let client = reqwest::Client::new();
    let mut request = client
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .body(body);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.send().await.expect("send")
}

pub async fn post(
    base: &str,
    path: &str,
    key: Option<&str>,
    body: serde_json::Value,
) -> reqwest::Response {
    let client = reqwest::Client::new();
    let mut request = client
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .body(serde_json::to_vec(&body).expect("serialize body"));
    if let Some(key) = key {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    request.send().await.expect("send")
}

/// 读取响应体并解析为 JSON（不依赖 reqwest 的 `json` feature）。
pub async fn parse_json(response: reqwest::Response) -> serde_json::Value {
    let text = response.text().await.expect("read body");
    serde_json::from_str(&text).expect("parse json")
}

pub async fn get(base: &str, path: &str, key: Option<&str>) -> reqwest::Response {
    let client = reqwest::Client::new();
    let mut request = client.get(format!("{base}{path}"));
    if let Some(key) = key {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    request.send().await.expect("send")
}
