//! 转发引擎：鉴权（key → route）→ 端点分类 → 模型校验 → 选号 → 保真转发（含换号）。
//!
//! 移植自 develop 的 `forward.rs`，改动：无租户/池段（key 定位路由）、`model` 原样透传、
//! 结果经 [`CoreEvent`] 外发、无任何持久化。

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant, SystemTime};

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use futures_util::{Stream, StreamExt};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::affinity::Affinity;
use crate::config::{AffinitySeed, ApiFormat, CoreConfig, CoreOptions};
use crate::decode::{BodyDecoder, ContentEncoding, decode_bytes};
use crate::event::{
    AccessEvent, AttemptEvent, CompletionEvent, CompletionOutcome, CoreEvent, EventBus,
};
use crate::health::{
    Classification, Cooldowns, Failure, HealthManager, Probing, TransportError, begin_probe,
    classify, epoch_secs,
};
use crate::inbound::{Endpoint, Family, Inbound, clean_query, upstream_url};
use crate::metrics::Metrics;
use crate::registry::{ProviderRuntime, ProviderSnapshot, Registry, RouteRuntime, Selector};
use crate::usage::{TokenUsage, extract_usage_from_json, extract_usage_from_sse_data};

const ERROR_TYPE: &str = "invalid_request_error";
/// Anthropic Messages 的必需版本头（探针与客户端转发都依赖它）。
const ANTHROPIC_VERSION: &str = "2023-06-01";
const ERROR_PREFIX_LIMIT: usize = 64 * 1024;
const PROBE_BODY_LIMIT: usize = 8 * 1024;

/// 引擎构建 / 配置替换失败。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// 上游 HTTP client 构建失败。
    #[error("上游 HTTP client 构建失败: {0}")]
    Client(#[from] reqwest::Error),
}

/// 网关自造错误码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayErrorCode {
    /// 凭据无效或已停用 → 401。
    InvalidApiKey,
    /// 非工作端点 → 404。
    UnsupportedEndpoint,
    /// 路由不存在或已停用 → 404。
    RouteNotFound,
    /// `model` 缺失、不可解析或该路由无凭据可服务 → 404。
    ModelNotFound,
    /// 模型存在但没有任何 Provider 声明支持当前入站 API 格式 → 404。
    UnsupportedApiFormat,
    /// 请求体超限 → 413。
    RequestTooLarge,
    /// 路由内无健康 provider 可承载该模型 → 503。
    RouteExhausted,
    /// 请求体读取中途失败 → 400。
    RequestBodyIncomplete,
}

impl GatewayErrorCode {
    pub fn status(self) -> StatusCode {
        match self {
            GatewayErrorCode::InvalidApiKey => StatusCode::UNAUTHORIZED,
            GatewayErrorCode::UnsupportedEndpoint
            | GatewayErrorCode::RouteNotFound
            | GatewayErrorCode::ModelNotFound
            | GatewayErrorCode::UnsupportedApiFormat => StatusCode::NOT_FOUND,
            GatewayErrorCode::RequestTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            GatewayErrorCode::RouteExhausted => StatusCode::SERVICE_UNAVAILABLE,
            GatewayErrorCode::RequestBodyIncomplete => StatusCode::BAD_REQUEST,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            GatewayErrorCode::InvalidApiKey => "invalid_api_key",
            GatewayErrorCode::UnsupportedEndpoint => "unsupported_endpoint",
            GatewayErrorCode::RouteNotFound => "route_not_found",
            GatewayErrorCode::ModelNotFound => "model_not_found",
            GatewayErrorCode::UnsupportedApiFormat => "unsupported_api_format",
            GatewayErrorCode::RequestTooLarge => "request_too_large",
            GatewayErrorCode::RouteExhausted => "route_exhausted",
            GatewayErrorCode::RequestBodyIncomplete => "request_body_incomplete",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            GatewayErrorCode::InvalidApiKey => "Incorrect API key provided.",
            GatewayErrorCode::UnsupportedEndpoint => {
                "This endpoint is not supported by the gateway."
            }
            GatewayErrorCode::RouteNotFound => "The requested route does not exist.",
            GatewayErrorCode::ModelNotFound => {
                "The requested model is not available in this route."
            }
            GatewayErrorCode::UnsupportedApiFormat => {
                "The requested API format is not supported for this model in this route."
            }
            GatewayErrorCode::RequestTooLarge => "Request body is too large.",
            GatewayErrorCode::RouteExhausted => {
                "No healthy provider is available for this model in this route."
            }
            GatewayErrorCode::RequestBodyIncomplete => "Request body could not be read completely.",
        }
    }

    /// 自造错误体按入站家族成形：错误的消费者是客户端，客户端用什么协议调用就用什么形状回话。
    pub fn into_response(self, family: Family) -> Response {
        (self.status(), axum::Json(self.error_body(family))).into_response()
    }

    /// 自造错误体（按家族成形），单测直接断言形状。
    pub fn error_body(self, family: Family) -> Value {
        match family {
            Family::OpenAi => json!({
                "error": {
                    "message": self.message(),
                    "type": ERROR_TYPE,
                    "code": self.code(),
                }
            }),
            Family::Anthropic => json!({
                "type": "error",
                "error": {
                    "type": self.anthropic_error_type(),
                    "message": self.message(),
                    "code": self.code(),
                }
            }),
        }
    }

    fn anthropic_error_type(self) -> &'static str {
        match self {
            GatewayErrorCode::InvalidApiKey => "authentication_error",
            GatewayErrorCode::UnsupportedEndpoint
            | GatewayErrorCode::RouteNotFound
            | GatewayErrorCode::ModelNotFound
            | GatewayErrorCode::UnsupportedApiFormat => "not_found_error",
            GatewayErrorCode::RequestTooLarge => "request_too_large",
            GatewayErrorCode::RouteExhausted => "api_error",
            GatewayErrorCode::RequestBodyIncomplete => "invalid_request_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Payload {
    model: String,
    session: Option<String>,
}

/// 从请求体顶层 `model` 取客户端模型名，并按入站端点提取会话标识。
fn parse_payload(inbound: Inbound, raw: &[u8]) -> Option<Payload> {
    let value: Value = serde_json::from_slice(raw).ok()?;
    let object = value.as_object()?;
    let model = object.get("model").and_then(Value::as_str)?.to_string();
    Some(Payload {
        model,
        session: session_key(inbound, object),
    })
}

fn session_key(inbound: Inbound, object: &serde_json::Map<String, Value>) -> Option<String> {
    match inbound {
        Inbound::Responses => [
            "previous_response_id",
            "conversation",
            "prompt_cache_key",
            "user",
        ]
        .iter()
        .find_map(|field| field_value(object.get(*field))),
        Inbound::ChatCompletions => ["prompt_cache_key", "user"]
            .iter()
            .find_map(|field| field_value(object.get(*field))),
        // Anthropic 的会话标识在 `metadata.user_id`。
        Inbound::Messages => object
            .get("metadata")
            .and_then(|metadata| metadata.get("user_id"))
            .and_then(value_as_string),
        Inbound::MessagesCountTokens => None,
    }
}

fn field_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Object(object) => object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_string),
        _ => None,
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

fn models_response(route: &RouteRuntime) -> Response {
    let models = route.models();
    // OpenAI list 形状 + Anthropic 可识别的扩展字段。Anthropic SDK 的 models.list()
    // 也调用 GET /v1/models；两边都不认识的字段会被忽略，因此用并集形状。
    let data: Vec<Value> = models
        .iter()
        .map(|model| {
            json!({
                "id": model,
                "object": "model",
                "created": 0,
                "owned_by": route.name,
                "type": "model",
                "display_name": model,
                "created_at": "1970-01-01T00:00:00Z",
            })
        })
        .collect();
    axum::Json(json!({
        "object": "list",
        "data": data,
        "has_more": false,
        "first_id": models.first(),
        "last_id": models.last(),
    }))
    .into_response()
}

fn method_not_allowed(allow: &'static str) -> Response {
    let mut response = StatusCode::METHOD_NOT_ALLOWED.into_response();
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static(allow));
    response
}

fn build_client(options: &CoreOptions) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(options.connect_timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
}

/// 健康检查应答（网关本地端点）。
pub async fn health() -> &'static str {
    "ok"
}

/// 统一转发入口（`/v1/*` 全部路径）。
async fn dispatch(
    State(core): State<Arc<Core>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    core.handle(method, uri, headers, body).await
}

/// API Key 哈希（sha256 hex）；控制面用同一算法生成 `key_hash`。
pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// 引擎的可替换状态：路由/凭据索引 + 运行参数。
struct EngineState {
    registry: Registry,
    selector: Selector,
    options: CoreOptions,
}

/// 数据面引擎。
pub struct Core {
    state: RwLock<Arc<EngineState>>,
    health: Arc<HealthManager>,
    affinity: Arc<Affinity>,
    probing: Arc<Probing>,
    events: EventBus,
    client: reqwest::Client,
    probe_count: Arc<AtomicU64>,
    /// 选号指标（内存，不落库）。
    metrics: Arc<Metrics>,
    /// Google Service Account 实例缓存（按 provider id）。
    gcp_accounts:
        tokio::sync::Mutex<std::collections::HashMap<String, Arc<gcp_auth::CustomServiceAccount>>>,
}

impl std::fmt::Debug for Core {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let routes = self
            .state
            .read()
            .map(|s| s.registry.routes().count())
            .unwrap_or(0);
        f.debug_struct("Core")
            .field("routes", &routes)
            .finish_non_exhaustive()
    }
}

impl Core {
    /// 用一份完整配置启动引擎（含健康 / 选号指标 / 粘性种子）。
    pub async fn new(config: CoreConfig) -> Result<Arc<Self>, CoreError> {
        Self::build(config).await
    }

    async fn build(config: CoreConfig) -> Result<Arc<Self>, CoreError> {
        let events = EventBus::new();
        let health = HealthManager::restore(config.health.clone(), events.clone());
        let client = build_client(&config.options)?;
        let affinity = Arc::new(Affinity::new(config.options.affinity_ttl));
        let metrics = Arc::new(Metrics::new());
        let registry = Registry::new(&config);
        let selector = Selector::new(&registry, Arc::clone(&health), Arc::clone(&metrics));
        // 启动种子：指标与粘性都只在本地无记录时采用（运行期 apply_config 不带种子）。
        let now = epoch_secs(SystemTime::now());
        let mut seeded_metrics = 0usize;
        for seed in &config.metrics {
            if metrics.seed(
                &seed.provider_id,
                seed.ttfb_ms,
                seed.failure_rate,
                seed.samples,
                now,
            ) {
                seeded_metrics += 1;
            }
        }
        let seeded_affinity = affinity.seed(config.affinity.clone()).await;
        affinity
            .retain(|route, _model, provider| {
                registry.route(route).is_some() && registry.provider(provider).is_some()
            })
            .await;
        let state = EngineState {
            registry,
            selector,
            options: config.options.clone(),
        };
        if seeded_metrics > 0 || seeded_affinity > 0 {
            tracing::info!(
                metrics = seeded_metrics,
                affinity = seeded_affinity,
                "runtime state seeds restored"
            );
        }

        Ok(Arc::new(Self {
            state: RwLock::new(Arc::new(state)),
            health,
            affinity,
            probing: Arc::new(Probing::default()),
            events,
            client,
            probe_count: Arc::new(AtomicU64::new(0)),
            metrics,
            gcp_accounts: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }))
    }

    /// 原子替换配置（路由 / 凭据 / 运行参数）。健康不随替换丢失；
    /// 已删除 / 已归档资源的指标与粘性会被清理。
    pub async fn apply_config(&self, config: CoreConfig) -> Result<(), CoreError> {
        // R1-F-12①：此处与 `build` 各构造一次 registry，是两条独立生命周期
        //（启动装配 / 配置替换），并非重复调用；不合并。
        let registry = Registry::new(&config);
        // 已删除的 provider 的指标随配置替换丢弃，其余保留。
        self.metrics.retain(|id| registry.provider(id).is_some());
        self.affinity
            .retain(|route, _model, provider| {
                registry.route(route).is_some() && registry.provider(provider).is_some()
            })
            .await;
        let selector = Selector::new(
            &registry,
            Arc::clone(&self.health),
            Arc::clone(&self.metrics),
        );
        let state = EngineState {
            registry,
            selector,
            options: config.options.clone(),
        };
        *self.state.write().expect("state lock") = Arc::new(state);
        self.gcp_accounts.lock().await.clear();
        Ok(())
    }

    /// 订阅事件流。
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }

    pub fn probe_count(&self) -> u64 {
        self.probe_count.load(Ordering::Relaxed)
    }

    /// 只读导出未过期的会话粘性条目（进程收尾时落盘用）。
    pub async fn export_affinity(&self, limit: usize) -> Vec<AffinitySeed> {
        self.affinity.export(limit).await
    }

    /// 选号快照：某路由下每个 provider 的指标与当前分数（控制面只读展示用）。
    ///
    /// 返回 `None` 表示路由不存在。`model` 为 `None` 时取该路由模型并集的首项。
    pub async fn selection_snapshot(
        &self,
        route_id: &str,
        model: Option<&str>,
    ) -> Option<Vec<ProviderSnapshot>> {
        let state = {
            let guard = self.state.read().ok()?;
            Arc::clone(&guard)
        };
        let route = state.registry.route(route_id)?;
        Some(state.selector.snapshot(route, model).await)
    }

    /// 转发路由：`/health` + `/v1/*`。
    ///
    /// 所有 `/v1` 下的 path（含未定义者）都进同一入口，保证**鉴权先于端点分类**：
    /// 无效凭据 + 未定义端点必须返回 401，而非 404。
    pub fn router(self: &Arc<Self>) -> axum::Router {
        use axum::routing::{any, get};
        axum::Router::new()
            .route("/health", get(health))
            .route("/v1", any(dispatch))
            .route("/v1/{*rest}", any(dispatch))
            // Gemini 端点已移除；保留挂载让旧 path 返回 JSON 404，而不是落入 SPA fallback。
            .route("/v1beta", any(dispatch))
            .route("/v1beta/{*rest}", any(dispatch))
            .route("/upload", any(dispatch))
            .route("/upload/{*rest}", any(dispatch))
            .with_state(Arc::clone(self))
    }

    fn cooldowns(&self) -> Cooldowns {
        self.state
            .read()
            .map(|s| s.options.cooldowns)
            .unwrap_or(Cooldowns {
                auth: Duration::from_secs(0),
                rate_limit: Duration::from_secs(0),
                unavailable: Duration::from_secs(0),
                quota: Duration::from_secs(0),
            })
    }

    /// 入站处理：鉴权 401 → 端点 404 → method 405 → 读体 413/400 → model 404 → 换号 503。
    async fn handle(
        self: Arc<Self>,
        method: Method,
        uri: Uri,
        headers: HeaderMap,
        body: Body,
    ) -> Response {
        // 错误体外壳只由 path 字面量决定，因此鉴权失败时也能成形。
        let family = Family::from_path(uri.path());

        let state = match self.state.read() {
            Ok(state) => Arc::clone(&state),
            Err(_) => return GatewayErrorCode::InvalidApiKey.into_response(family),
        };

        // 鉴权：API Key 哈希 → 路由。
        let Some((api_key_id, route_id)) = authenticate(&state, &headers) else {
            return GatewayErrorCode::InvalidApiKey.into_response(family);
        };
        let Some(route) = state.registry.route(&route_id) else {
            return GatewayErrorCode::InvalidApiKey.into_response(family);
        };
        if !route.enabled {
            return GatewayErrorCode::RouteNotFound.into_response(family);
        }

        let Some(endpoint) = Endpoint::from_path(uri.path()) else {
            return GatewayErrorCode::UnsupportedEndpoint.into_response(family);
        };

        if !endpoint.allows_method(&method) {
            return method_not_allowed(endpoint.allow_header());
        }

        match endpoint {
            Endpoint::Models => models_response(route),
            Endpoint::Forward(target) => {
                let raw = match read_body_limited(body, state.options.request_body_limit).await {
                    Ok(raw) => raw,
                    Err(ReadBodyError::TooLarge) => {
                        return GatewayErrorCode::RequestTooLarge.into_response(family);
                    }
                    Err(ReadBodyError::Io) => {
                        return GatewayErrorCode::RequestBodyIncomplete.into_response(family);
                    }
                };
                let Some(payload) = parse_payload(target, &raw) else {
                    return GatewayErrorCode::ModelNotFound.into_response(family);
                };
                if !route.has_model(&payload.model) {
                    return GatewayErrorCode::ModelNotFound.into_response(family);
                }
                let format = target.api_format();
                if !route.supports_format(&payload.model, format) {
                    return GatewayErrorCode::UnsupportedApiFormat.into_response(family);
                }
                let request_id = uuid_like();
                self.forward(
                    &state,
                    request_id,
                    &api_key_id,
                    route,
                    &method,
                    &uri,
                    &headers,
                    raw,
                    payload,
                    target,
                )
                .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn forward(
        &self,
        state: &EngineState,
        request_id: String,
        api_key_id: &str,
        route: &RouteRuntime,
        method: &Method,
        uri: &Uri,
        headers: &HeaderMap,
        raw: Bytes,
        payload: Payload,
        target: Inbound,
    ) -> Response {
        let started = Instant::now();
        let at = epoch_secs(SystemTime::now());
        let model = payload.model.as_str();
        let bytes_in = raw.len() as u64;
        let format = target.api_format();
        let family = target.family();
        let api_format = format.as_str();
        let generation = target.is_generation();
        // OpenAI 家族流式默认不回传 usage（需 `stream_options.include_usage`）：
        // 缺失时补上，否则 token 统计恒为 0。Anthropic 原生流与 Responses 本就带 usage。
        let want_stream_usage = matches!(target, Inbound::ChatCompletions);
        let client_path = uri.path().to_string();
        // 凭据类 query 参数（`key` / `api_key`）绝不外传；其余原样保留。
        let query = clean_query(uri.query().unwrap_or_default());

        let affinity_provider = match &payload.session {
            Some(session) => self.affinity.lookup(&route.id, model, session).await,
            None => None,
        };
        self.spawn_probes(state, route, model).await;
        let pick = state
            .selector
            .pick(
                route,
                model,
                format,
                affinity_provider.as_deref(),
                &self.probing,
            )
            .await;

        let mut tried: HashSet<String> = HashSet::new();
        let mut attempts = 0u32;
        let mut last_failure: Option<LastFailure> = None;

        for provider_id in pick.order() {
            if !tried.insert(provider_id.clone()) {
                continue;
            }
            let Some(provider) = state.registry.provider(&provider_id).cloned() else {
                continue;
            };
            let Some(upstream_model) = provider.upstream_model(model) else {
                continue;
            };
            attempts += 1;
            let attempt_start = Instant::now();
            let rewrite_alias = (upstream_model != model).then(|| model.to_string());
            // 模型别名生效时只改请求体顶层 `model`；同名完全不碰。
            let (mut body, path) = match &rewrite_alias {
                Some(_) => (
                    Bytes::from(rewrite_top_level_string(&raw, "model", upstream_model)),
                    client_path.clone(),
                ),
                None => (raw.clone(), client_path.clone()),
            };
            // 上游路径 = 凭据上的 `path_template`（含路径参数 / model / action / endpoint）渲染结果；
            // 没有模板时回退到 `{base_url}{家族后缀}`。路径参数属于凭据，不能来自客户端 URL —— 这是池化的前提。
            let rendered = render_target(
                &provider,
                target,
                family,
                &path,
                body.clone(),
                upstream_model,
            );
            body = rendered.body;
            if want_stream_usage {
                body = ensure_stream_usage(&body);
            }
            let base = provider.base_url.trim_end_matches('/');
            let url = match rendered.path {
                Some(target_path) => join_target(base, &target_path, query.as_deref()),
                None => upstream_url(base, &path, query.as_deref(), family),
            };

            match self
                .attempt(state, &provider, method, url, headers, body)
                .await
            {
                Attempt::Reached(upstream) => {
                    let status = upstream.status.as_u16();
                    self.emit_attempt(
                        &request_id,
                        &route.id,
                        model,
                        &provider_id,
                        Some(status),
                        None,
                        attempt_start.elapsed().as_millis() as u64,
                    );
                    if let Some(session) = &payload.session {
                        self.affinity
                            .remember(&route.id, model, session, &provider_id)
                            .await;
                    }
                    let ttft = attempt_start.elapsed().as_millis() as u64;
                    self.metrics
                        .record_success(&provider_id, ttft, epoch_secs(SystemTime::now()));
                    let is_stream = upstream
                        .headers
                        .get(header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .is_some_and(|value| {
                            value.to_ascii_lowercase().starts_with("text/event-stream")
                        });
                    self.emit_access(
                        &request_id,
                        at,
                        route,
                        api_key_id,
                        model,
                        &provider_id,
                        api_format,
                        is_stream,
                        status,
                        attempts,
                        ttft,
                        bytes_in,
                    );
                    let meta = CompletionMeta {
                        request_id,
                        at,
                        route_id: route.id.clone(),
                        api_key_id: api_key_id.to_string(),
                        model: model.to_string(),
                        provider_id,
                        started,
                        generation,
                    };
                    let alias = rewrite_alias.map(|alias| AliasRewrite { alias, family });
                    return self
                        .success_response(upstream, meta, alias, &state.options)
                        .await;
                }
                Attempt::Failed {
                    status,
                    headers,
                    classification,
                    body,
                } => {
                    self.emit_attempt(
                        &request_id,
                        &route.id,
                        model,
                        &provider_id,
                        Some(status.as_u16()),
                        Some(classification_reason(classification)),
                        attempt_start.elapsed().as_millis() as u64,
                    );
                    self.apply_classification(&provider_id, classification)
                        .await;
                    if !classification.is_marked() {
                        // 请求级错误：provider 无问题，原样返回且不换号。
                        self.emit_access(
                            &request_id,
                            at,
                            route,
                            api_key_id,
                            model,
                            &provider_id,
                            api_format,
                            false,
                            status.as_u16(),
                            attempts,
                            started.elapsed().as_millis() as u64,
                            bytes_in,
                        );
                        // R1-F-8：错误响应体逐字节透传，MUST NOT 回写 model。
                        return LastFailure::from_body(status, headers, body).into_response(None);
                    }
                    self.metrics
                        .record_failure(&provider_id, epoch_secs(SystemTime::now()));
                    last_failure = Some(LastFailure::from_body(status, headers, body));
                }
                Attempt::Transport {
                    error,
                    classification,
                } => {
                    tracing::warn!(provider = %provider_id, error = %error, "upstream transport failure");
                    self.metrics
                        .record_failure(&provider_id, epoch_secs(SystemTime::now()));
                    self.emit_attempt(
                        &request_id,
                        &route.id,
                        model,
                        &provider_id,
                        None,
                        Some(error.to_string()),
                        attempt_start.elapsed().as_millis() as u64,
                    );
                    self.apply_classification(&provider_id, classification)
                        .await;
                }
            }
        }

        match last_failure {
            Some(failure) => {
                let status = failure.status.as_u16();
                self.emit_access(
                    &request_id,
                    at,
                    route,
                    api_key_id,
                    model,
                    // R1-F-2：全败无承接，provider_id 为空（对齐 management-api 契约）。
                    "",
                    api_format,
                    false,
                    status,
                    attempts,
                    started.elapsed().as_millis() as u64,
                    bytes_in,
                );
                // R1-F-8：LastFailure 透传的上游错误体逐字节保留，不回写 model。
                failure.into_response(None)
            }
            None => {
                self.emit_access(
                    &request_id,
                    at,
                    route,
                    api_key_id,
                    model,
                    "",
                    api_format,
                    false,
                    GatewayErrorCode::RouteExhausted.status().as_u16(),
                    attempts,
                    started.elapsed().as_millis() as u64,
                    bytes_in,
                );
                GatewayErrorCode::RouteExhausted.into_response(family)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn attempt(
        &self,
        state: &EngineState,
        provider: &ProviderRuntime,
        method: &Method,
        url: String,
        headers: &HeaderMap,
        body: Bytes,
    ) -> Attempt {
        let request = match self
            .build_request(
                method,
                &url,
                headers,
                body,
                &provider.api_key,
                &provider.auth_scheme,
                provider,
            )
            .await
        {
            Ok(request) => request,
            Err(error) => {
                let transport = TransportError::Other(error);
                let classification = classify(
                    Failure::Transport(transport.clone()),
                    self.cooldowns(),
                    SystemTime::now(),
                );
                return Attempt::Transport {
                    error: transport,
                    classification,
                };
            }
        };
        let response =
            match tokio::time::timeout(state.options.first_byte_timeout, request.send()).await {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    let transport = transport_error(&error);
                    let classification = classify(
                        Failure::Transport(transport.clone()),
                        self.cooldowns(),
                        SystemTime::now(),
                    );
                    return Attempt::Transport {
                        error: transport,
                        classification,
                    };
                }
                Err(_) => {
                    let transport = TransportError::Timeout(format!(
                        "首字节超时（{}ms）",
                        state.options.first_byte_timeout.as_millis()
                    ));
                    let classification = classify(
                        Failure::Transport(transport.clone()),
                        self.cooldowns(),
                        SystemTime::now(),
                    );
                    return Attempt::Transport {
                        error: transport,
                        classification,
                    };
                }
            };
        self.read_response(response, state.options.response_body_timeout)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_request(
        &self,
        method: &Method,
        url: &str,
        headers: &HeaderMap,
        body: Bytes,
        api_key: &str,
        provider_auth_scheme: &str,
        provider: &ProviderRuntime,
    ) -> Result<reqwest::RequestBuilder, String> {
        let reqwest_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
            .unwrap_or(reqwest::Method::POST);
        let mut forwarded = HeaderMap::new();
        // 白名单改写之一：鉴权头按 provider 预设的鉴权方式替换。
        forwarded.extend(
            self.auth_headers(api_key, provider_auth_scheme, provider)
                .await?,
        );
        for (name, value) in headers {
            let name_str = name.as_str();
            if is_dropped_header(name_str)
                || name_str.eq_ignore_ascii_case("host")
                || name_str.eq_ignore_ascii_case("authorization")
                || name_str.eq_ignore_ascii_case("x-api-key")
                || name_str.eq_ignore_ascii_case("x-goog-api-key")
                || name_str.eq_ignore_ascii_case("api-key")
            {
                continue;
            }
            forwarded.append(name.clone(), value.clone());
        }
        // SigV4：签名覆盖最终 URL、参与签名的 header 与最终请求体。
        if provider_auth_scheme == "sigv4" {
            let content_type = forwarded
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let signed = sign_provider_request(
                provider,
                method.as_str(),
                url,
                content_type.as_deref(),
                &body,
            )?;
            forwarded.insert(
                header::HOST,
                HeaderValue::from_str(&signed.host).map_err(|_| "Host 头非法".to_string())?,
            );
            forwarded.insert(
                HeaderName::from_static("x-amz-date"),
                HeaderValue::from_str(&signed.amz_date)
                    .map_err(|_| "x-amz-date 头非法".to_string())?,
            );
            if let Some(token) = &signed.session_token {
                forwarded.insert(
                    HeaderName::from_static("x-amz-security-token"),
                    HeaderValue::from_str(token).map_err(|_| "会话令牌头非法".to_string())?,
                );
            }
            forwarded.insert(
                header::AUTHORIZATION,
                HeaderValue::from_str(&signed.authorization)
                    .map_err(|_| "Authorization 头非法".to_string())?,
            );
        }
        Ok(self
            .client
            .request(reqwest_method, url)
            .headers(forwarded)
            .body(body))
    }

    async fn auth_headers(
        &self,
        api_key: &str,
        scheme: &str,
        provider: &ProviderRuntime,
    ) -> Result<HeaderMap, String> {
        if scheme != "google_service_account" {
            return Ok(auth_headers(api_key, scheme));
        }
        let account = {
            let mut accounts = self.gcp_accounts.lock().await;
            if let Some(account) = accounts.get(&provider.id) {
                Arc::clone(account)
            } else {
                let account = Arc::new(
                    gcp_auth::CustomServiceAccount::from_json(api_key)
                        .map_err(|error| format!("Service Account JSON 无效: {error}"))?,
                );
                accounts.insert(provider.id.clone(), Arc::clone(&account));
                account
            }
        };
        use gcp_auth::TokenProvider;
        let token = account
            .token(&["https://www.googleapis.com/auth/cloud-platform"])
            .await
            .map_err(|error| format!("获取 Google OAuth2 令牌失败: {error}"))?;
        let mut headers = HeaderMap::new();
        let value = HeaderValue::from_str(&format!("Bearer {}", token.as_str()))
            .map_err(|error| format!("OAuth2 令牌无法作为 header: {error}"))?;
        headers.insert(header::AUTHORIZATION, value);
        Ok(headers)
    }

    async fn read_response(&self, response: reqwest::Response, body_timeout: Duration) -> Attempt {
        let status = response.status();
        let headers = response.headers().clone();
        if status.is_success() {
            return Attempt::Reached(Upstream {
                status,
                headers,
                body: response,
            });
        }

        let retry_after = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let mut response = response;
        let mut prefix: Vec<u8> = Vec::new();
        loop {
            if prefix.len() >= ERROR_PREFIX_LIMIT {
                return self.failed(
                    status,
                    headers,
                    retry_after,
                    prefix,
                    Some((Bytes::new(), response)),
                );
            }
            match next_chunk(&mut response, body_timeout).await {
                ChunkRead::Data(chunk) => {
                    let remaining = ERROR_PREFIX_LIMIT - prefix.len();
                    if chunk.len() > remaining {
                        prefix.extend_from_slice(&chunk[..remaining]);
                        let tail = chunk.slice(remaining..);
                        return self.failed(
                            status,
                            headers,
                            retry_after,
                            prefix,
                            Some((tail, response)),
                        );
                    }
                    prefix.extend_from_slice(&chunk);
                }
                ChunkRead::Eof => return self.failed(status, headers, retry_after, prefix, None),
                ChunkRead::Io(error) => {
                    let transport = transport_error(&error);
                    let classification = classify(
                        Failure::Transport(transport.clone()),
                        self.cooldowns(),
                        SystemTime::now(),
                    );
                    return Attempt::Transport {
                        error: transport,
                        classification,
                    };
                }
                ChunkRead::Timeout => {
                    let transport = TransportError::Timeout("上游错误响应体读取超时".to_string());
                    let classification = classify(
                        Failure::Transport(transport.clone()),
                        self.cooldowns(),
                        SystemTime::now(),
                    );
                    return Attempt::Transport {
                        error: transport,
                        classification,
                    };
                }
            }
        }
    }

    fn failed(
        &self,
        status: StatusCode,
        headers: HeaderMap,
        retry_after: Option<String>,
        prefix: Vec<u8>,
        rest: Option<(Bytes, reqwest::Response)>,
    ) -> Attempt {
        let classification = classify(
            Failure::Response {
                status: status.as_u16(),
                retry_after: retry_after.as_deref(),
                body_prefix: &prefix,
            },
            self.cooldowns(),
            SystemTime::now(),
        );
        let body = match rest {
            Some((tail, response)) => {
                let mut combined = prefix;
                combined.extend_from_slice(&tail);
                Some(PrefixedTail {
                    prefix: combined,
                    rest: Some(response),
                })
            }
            None => Some(PrefixedTail { prefix, rest: None }),
        };
        Attempt::Failed {
            status,
            headers,
            classification,
            body,
        }
    }

    async fn apply_classification(&self, provider_id: &str, classification: Classification) {
        let Classification::Marked { mark, reason } = classification else {
            return;
        };
        self.health
            .apply(provider_id, mark, reason, epoch_secs(SystemTime::now()))
            .await;
    }

    async fn spawn_probes(&self, state: &EngineState, route: &RouteRuntime, model: &str) {
        let client = self.client.clone();
        let health = Arc::clone(&self.health);
        let cooldowns = state.options.cooldowns;
        let timeout = state.options.first_byte_timeout;
        let body_timeout = state.options.response_body_timeout;
        for provider_id in state
            .selector
            .probe_targets(route, model, &self.probing)
            .await
        {
            let Some(guard) = begin_probe(&self.probing, &provider_id).await else {
                continue;
            };
            let Some(provider) = state.registry.provider(&provider_id).cloned() else {
                continue;
            };
            // 探针目前只支持静态凭据；Service Account 需要异步 OAuth2 token，
            // 在探针路径接入前跳过，避免把正常 provider 误判为鉴权失败。
            if provider.credential_kind == "google_service_account" {
                continue;
            }
            self.probe_count.fetch_add(1, Ordering::Relaxed);
            let (client, health) = (client.clone(), Arc::clone(&health));
            tokio::spawn(async move {
                probe_provider(
                    &client,
                    &health,
                    &provider,
                    cooldowns,
                    timeout,
                    body_timeout,
                )
                .await;
                drop(guard);
            });
        }
    }

    fn emit_attempt(
        &self,
        request_id: &str,
        route_id: &str,
        model: &str,
        provider_id: &str,
        status: Option<u16>,
        error: Option<String>,
        latency_ms: u64,
    ) {
        self.events.emit(CoreEvent::Attempt(AttemptEvent {
            request_id: request_id.to_string(),
            at: epoch_secs(SystemTime::now()),
            route_id: route_id.to_string(),
            model: model.to_string(),
            provider_id: provider_id.to_string(),
            status,
            error,
            latency_ms,
        }));
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_access(
        &self,
        request_id: &str,
        at: i64,
        route: &RouteRuntime,
        api_key_id: &str,
        model: &str,
        provider_id: &str,
        api_format: &str,
        is_stream: bool,
        status: u16,
        attempts: u32,
        ttfb_ms: u64,
        bytes_in: u64,
    ) {
        self.events.emit(CoreEvent::Access(AccessEvent {
            request_id: request_id.to_string(),
            at,
            route_id: route.id.clone(),
            api_key_id: api_key_id.to_string(),
            model: model.to_string(),
            provider_id: provider_id.to_string(),
            api_format: api_format.to_string(),
            is_stream,
            status,
            attempts,
            ttfb_ms,
            bytes_in,
        }));
    }

    fn emit_completion(
        &self,
        meta: &CompletionMeta,
        outcome: CompletionOutcome,
        bytes_out: u64,
        usage: Option<TokenUsage>,
    ) {
        self.events.emit(CoreEvent::Completion(CompletionEvent {
            request_id: meta.request_id.clone(),
            at: meta.at,
            route_id: meta.route_id.clone(),
            api_key_id: meta.api_key_id.clone(),
            model: meta.model.clone(),
            provider_id: meta.provider_id.clone(),
            outcome,
            duration_ms: meta.started.elapsed().as_millis() as u64,
            bytes_out,
            usage,
            generation: meta.generation,
        }));
    }

    async fn success_response(
        &self,
        upstream: Upstream,
        meta: CompletionMeta,
        alias: Option<AliasRewrite>,
        options: &CoreOptions,
    ) -> Response {
        let Upstream {
            status,
            headers,
            mut body,
        } = upstream;
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_ascii_lowercase())
            .unwrap_or_default();
        let is_sse = content_type.starts_with("text/event-stream");
        // 压缩只影响旁路解析：转发字节按原样透传，`content-encoding` 也照原回给客户端。
        let content_encoding = headers
            .get(header::CONTENT_ENCODING)
            .and_then(|value| value.to_str().ok())
            .map(ContentEncoding::parse)
            .unwrap_or_default();
        // Bedrock 的流式响应是二进制事件流（非 SSE）：只透传、不解析 usage。
        let is_event_stream = content_type.starts_with("application/vnd.amazon.eventstream");

        let body = if is_sse {
            let base: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> =
                match alias {
                    Some(alias) => Box::pin(rewrite_sse_stream(body.bytes_stream(), alias)),
                    None => Box::pin(
                        body.bytes_stream()
                            .map(|chunk| chunk.map_err(std::io::Error::other)),
                    ),
                };
            Body::from_stream(ObservedSseStream {
                inner: base,
                pending: Vec::new(),
                decoder: (!content_encoding.is_identity())
                    .then(|| BodyDecoder::new(content_encoding)),
                reporter: StreamReporter::new(self.events.clone(), meta),
            })
        } else if is_event_stream {
            let base: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>> = Box::pin(
                body.bytes_stream()
                    .map(|chunk| chunk.map_err(std::io::Error::other)),
            );
            Body::from_stream(ObservedJsonStream {
                inner: base,
                reporter: StreamReporter::new(self.events.clone(), meta),
            })
        } else {
            let result = read_response_body_limited(
                &mut body,
                options.response_body_limit,
                options.response_body_timeout,
            )
            .await;
            match result {
                Ok(bytes) => {
                    // usage 只在旁路解码后的明文上解析；发给客户端的仍是原始字节。
                    let parsed = match content_encoding {
                        ContentEncoding::Identity => serde_json::from_slice::<Value>(&bytes).ok(),
                        kind => decode_bytes(kind, &bytes)
                            .and_then(|raw| serde_json::from_slice::<Value>(&raw).ok()),
                    };
                    let usage = parsed.as_ref().and_then(extract_usage_from_json);
                    let bytes_out = bytes.len() as u64;
                    let bytes = match &alias {
                        Some(alias) => rewrite_alias_json(&bytes, alias),
                        None => bytes.to_vec(),
                    };
                    self.emit_completion(&meta, CompletionOutcome::Completed, bytes_out, usage);
                    Body::from(bytes)
                }
                Err(error) => {
                    let reason = match &error {
                        ResponseBodyError::TooLarge => "response body exceeds limit",
                        ResponseBodyError::Timeout => "response body read timed out",
                        ResponseBodyError::Io => "failed to read upstream response body",
                    };
                    tracing::warn!(reason, "upstream response body rejected");
                    self.emit_completion(&meta, CompletionOutcome::UpstreamError, 0, None);
                    Body::empty()
                }
            }
        };

        let mut response = Response::new(body);
        *response.status_mut() = status;
        copy_response_headers(response.headers_mut(), &headers);
        response
    }
}

/// 入站鉴权头的固定尝试顺序：OpenAI 家族 SDK、Anthropic SDK。
///
/// 不按端点家族限定（鉴权先于端点分类），取第一个哈希命中者。
fn inbound_tokens(headers: &HeaderMap) -> [Option<&str>; 2] {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim());
    let x_api_key = headers
        .get("x-api-key")
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    [bearer, x_api_key]
}

/// 校验入站凭据并定位路由。
fn authenticate(state: &EngineState, headers: &HeaderMap) -> Option<(String, String)> {
    let now = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|delta| delta.as_secs() as i64)
        .unwrap_or(0);
    for token in inbound_tokens(headers).into_iter().flatten() {
        if token.is_empty() {
            continue;
        }
        let Some(binding) = state.registry.key_binding(&hash_api_key(token)) else {
            continue;
        };
        if let Some(expires_at) = binding.expires_at
            && now >= expires_at
        {
            continue;
        }
        return Some((binding.api_key_id.clone(), binding.route_id.clone()));
    }
    None
}

/// 失败原因文本（用于 `AttemptEvent.error`）。
fn classification_reason(classification: Classification) -> String {
    match classification {
        Classification::Marked { reason, .. } => reason.as_str().to_string(),
        Classification::Success => "success".to_string(),
        Classification::Unmarked => "unmarked".to_string(),
    }
}

/// 简易唯一请求 id（时间戳 + 计数）。
fn uuid_like() -> String {
    use std::sync::atomic::AtomicU64;
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let ms = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("req_{ms:x}{seq:x}")
}

/// 一次转发目标的构造结果：请求体（按需改写）与可选的自定义路径。
struct RenderedTarget {
    body: Bytes,
    /// `Some` = 由 `path_template` 渲染出的上游路径；`None` = 走默认的家族后缀拼接。
    path: Option<String>,
}

fn option_str<'a>(options: &'a Option<Value>, key: &str) -> Option<&'a str> {
    options
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn option_map<'a>(options: &'a Option<Value>, key: &str) -> Option<&'a serde_json::Map<String, Value>> {
    options
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_object)
}

/// 路径模板中 `{action}` 的取值。
///
/// 优先取预设声明的 `path_actions`（`unary` / `stream`）；未声明时 Anthropic 族按
/// `rawPredict` / `streamRawPredict` 解析。
fn resolve_action(
    provider: &ProviderRuntime,
    inbound: Inbound,
    body: &[u8],
) -> Option<String> {
    let actions = option_map(&provider.options, "path_actions");
    let stream = body_stream_flag(body);
    match inbound {
        Inbound::Messages | Inbound::MessagesCountTokens => {
            if let Some(actions) = actions
                && let Some(action) = actions
                    .get(if stream { "stream" } else { "unary" })
                    .and_then(Value::as_str)
            {
                return Some(action.to_string());
            }
            Some(
                if stream {
                    "streamRawPredict"
                } else {
                    "rawPredict"
                }
                .to_string(),
            )
        }
        _ => None,
    }
}

/// 按凭据上的 `path_template` 渲染上游路径；未声明模板时返回 `None`（走默认拼接）。
///
/// 模板占位符只允许三类：`path_params`（凭据级，写入时冻结）、`{model}` / `{action}` /
/// `{endpoint}`（请求级，运行期推导）。渲染后仍有未解析占位符时按无模板处理并告警。
fn render_target(
    provider: &ProviderRuntime,
    inbound: Inbound,
    family: Family,
    client_path: &str,
    body: Bytes,
    upstream_model: &str,
) -> RenderedTarget {
    let Some(template) = option_str(&provider.options, "path_template") else {
        // 无模板：正文保真（原生 Anthropic / OpenAI 兼容端点不需要 rawPredict 形状）。
        return RenderedTarget { body, path: None };
    };
    let action = resolve_action(provider, inbound, &body);

    // "模型在路径里"的 Anthropic 端点（Vertex rawPredict / Bedrock invoke）：
    // 去掉顶层 `model` 并注入 `anthropic_version`。契约来自模板而不是厂商字段。
    let body = if anthropic_target(inbound) && template.contains("{model}") {
        Bytes::from(rewrite_vertex_anthropic_body(
            &body,
            option_str(&provider.options, "anthropic_version").unwrap_or("vertex-2023-10-16"),
        ))
    } else {
        body
    };

    // 模板占位符可引用 `base_url_vars` 与 `path_params`：两者都是凭据级参数（如 location 同时
    // 出现在主机名与项目路径里），只在 UI 上填一次。
    let mut path = template.to_string();
    for key in ["base_url_vars", "path_params"] {
        if let Some(params) = option_map(&provider.options, key) {
            for (name, value) in params {
                if let Some(text) = value.as_str() {
                    path = path.replace(&format!("{{{name}}}"), text);
                }
            }
        }
    }
    let endpoint = client_path
        .strip_prefix(family.root())
        .unwrap_or(client_path)
        .trim_start_matches('/');
    path = path.replace("{endpoint}", endpoint);
    path = path.replace("{model}", upstream_model);
    if let Some(action) = action.as_deref() {
        path = path.replace("{action}", action);
    }

    if path.contains('{') {
        tracing::warn!(
            provider = %provider.id,
            template = %template,
            "path_template has unresolved placeholders; falling back to default path"
        );
        return RenderedTarget { body, path: None };
    }
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    RenderedTarget {
        body,
        path: Some(path),
    }
}

/// 拼接 `base_url` 与渲染后的路径，并把模板自带 query 与入站 query 合并。
fn join_target(base: &str, target_path: &str, inbound_query: Option<&str>) -> String {
    let (path, template_query) = match target_path.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target_path, None),
    };
    let mut url = format!("{base}{path}");
    let query = match (template_query, inbound_query) {
        (Some(template), Some(inbound)) if !inbound.is_empty() => Some(format!("{template}&{inbound}")),
        (Some(template), _) => Some(template.to_string()),
        (None, Some(inbound)) if !inbound.is_empty() => Some(inbound.to_string()),
        _ => None,
    };
    if let Some(query) = query.filter(|value| !value.is_empty()) {
        url.push('?');
        url.push_str(&query);
    }
    url
}

// 「Vertex 目标路径的构造」已被通用 `path_template` 取代；这里只保留 body 规则所需的判定。
fn anthropic_target(inbound: Inbound) -> bool {
    matches!(inbound, Inbound::Messages | Inbound::MessagesCountTokens)
}

fn body_stream_flag(raw: &[u8]) -> bool {
    serde_json::from_slice::<Value>(raw)
        .ok()
        .and_then(|value| value.get("stream").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn rewrite_vertex_anthropic_body(raw: &[u8], version: &str) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<Value>(raw) else {
        return raw.to_vec();
    };
    if let Some(object) = value.as_object_mut() {
        object.remove("model");
        object
            .entry("anthropic_version")
            .or_insert_with(|| Value::String(version.to_string()));
    }
    serde_json::to_vec(&value).unwrap_or_else(|_| raw.to_vec())
}

fn auth_headers(api_key: &str, scheme: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let ok = |value: &str| HeaderValue::from_str(value).ok();
    match scheme {
        "none" => {}
        // SigV4 需要请求级签名；在数据面实现之前绝不回退成 Bearer。
        "sigv4" => {}
        // Service Account 由异步路径换取 OAuth2 token，这里不做静态回退。
        "google_service_account" => {}
        "x-api-key" => {
            if let Some(value) = ok(api_key) {
                headers.insert("x-api-key", value);
            }
        }
        "api-key" => {
            if let Some(value) = ok(api_key) {
                headers.insert("api-key", value);
            }
        }
        _ => {
            if let Some(value) = ok(&format!("Bearer {api_key}")) {
                headers.insert(header::AUTHORIZATION, value);
            }
        }
    }
    headers
}

/// 用凭据给请求签名（SigV4）。region / service 来自冻结的 options。
fn sign_provider_request(
    provider: &ProviderRuntime,
    method: &str,
    url: &str,
    content_type: Option<&str>,
    body: &[u8],
) -> Result<crate::sigv4::SigV4Headers, String> {
    let credentials = crate::sigv4::AwsCredentials::parse(&provider.api_key)?;
    let region = option_str(&provider.options, "region")
        .or_else(|| {
            option_map(&provider.options, "base_url_vars")
                .and_then(|map| map.get("region"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| "缺少 region：SigV4 签名需要 region（端点变量 region）".to_string())?;
    let service = option_str(&provider.options, "aws_service").unwrap_or("bedrock");
    crate::sigv4::sign(
        &credentials,
        region,
        service,
        method,
        url,
        content_type,
        body,
        SystemTime::now(),
    )
}

/// 对单 provider 发起探针：极简请求，响应走同一张失败分类表。
async fn probe_provider(
    client: &reqwest::Client,
    health: &Arc<HealthManager>,
    provider: &ProviderRuntime,
    cooldowns: Cooldowns,
    timeout: Duration,
    body_timeout: Duration,
) {
    let Some(model) = provider.probe_model() else {
        return;
    };
    // 探针必须使用 Provider 真实支持的格式；不支持的格式不发探针。
    let Some(format) = provider.probe_format() else {
        return;
    };
    let (family, inbound, path, payload) = match format {
        ApiFormat::ChatCompletions => (
            Family::OpenAi,
            Inbound::ChatCompletions,
            "/v1/chat/completions".to_string(),
            json!({
                "model": model,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 1,
            }),
        ),
        ApiFormat::Responses => (
            Family::OpenAi,
            Inbound::Responses,
            "/v1/responses".to_string(),
            json!({
                "model": model,
                "input": "ping",
                "max_output_tokens": 16,
            }),
        ),
        ApiFormat::AnthropicMessages => (
            Family::Anthropic,
            Inbound::Messages,
            "/v1/messages".to_string(),
            json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{"role": "user", "content": "ping"}],
            }),
        ),
    };
    // 探针必须走与真实转发完全一致的地址构造（含 `path_template`），否则带模板的凭据
    // 会探测到不存在的路径并被误判为不可用。
    let rendered = render_target(
        provider,
        inbound,
        family,
        &path,
        Bytes::from(payload.to_string().into_bytes()),
        model,
    );
    let base = provider.base_url.trim_end_matches('/');
    let url = match rendered.path {
        Some(target_path) => join_target(base, &target_path, None),
        None => upstream_url(base, &path, None, family),
    };
    let mut headers = auth_headers(&provider.api_key, &provider.auth_scheme);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    if provider.auth_scheme == "sigv4"
        && let Ok(signed) = sign_provider_request(
            provider,
            "POST",
            &url,
            Some("application/json"),
            &rendered.body,
        )
    {
        for (name, value) in [
            (header::HOST.as_str(), signed.host),
            ("x-amz-date", signed.amz_date),
            (header::AUTHORIZATION.as_str(), signed.authorization),
        ] {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(&value),
            ) {
                headers.insert(name, value);
            }
        }
        if let Some(token) = signed.session_token
            && let Ok(value) = HeaderValue::from_str(&token)
        {
            headers.insert(HeaderName::from_static("x-amz-security-token"), value);
        }
    }
    let mut request = client.post(url).headers(headers);
    // Anthropic 的版本头是必需头，探针必须与真实转发一致。
    if format == ApiFormat::AnthropicMessages {
        request = request.header("anthropic-version", ANTHROPIC_VERSION);
    }
    let request = request.body(rendered.body);

    let classification = match tokio::time::timeout(timeout, request.send()).await {
        Ok(Ok(response)) => {
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .map(str::to_string);
            let mut response = response;
            let mut body: Vec<u8> = Vec::new();
            let mut read_error: Option<TransportError> = None;
            while body.len() < PROBE_BODY_LIMIT {
                match next_chunk(&mut response, body_timeout).await {
                    ChunkRead::Data(chunk) => {
                        let remaining = PROBE_BODY_LIMIT - body.len();
                        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                    }
                    ChunkRead::Eof => break,
                    ChunkRead::Io(error) => {
                        read_error = Some(transport_error(&error));
                        break;
                    }
                    ChunkRead::Timeout => {
                        read_error =
                            Some(TransportError::Timeout("探针响应体读取超时".to_string()));
                        break;
                    }
                }
            }
            match read_error {
                // 响应体读不完不能当作探针成功（200 + 挂起的 body 也必须失败）。
                Some(error) => classify(Failure::Transport(error), cooldowns, SystemTime::now()),
                None => classify(
                    Failure::Response {
                        status,
                        retry_after: retry_after.as_deref(),
                        body_prefix: &body,
                    },
                    cooldowns,
                    SystemTime::now(),
                ),
            }
        }
        Ok(Err(error)) => classify(
            Failure::Transport(transport_error(&error)),
            cooldowns,
            SystemTime::now(),
        ),
        Err(_) => classify(
            Failure::Transport(TransportError::Timeout("探针超时".to_string())),
            cooldowns,
            SystemTime::now(),
        ),
    };

    let now = epoch_secs(SystemTime::now());
    match classification {
        Classification::Success => health.mark_healthy(&provider.id, now).await,
        Classification::Marked { mark, reason } => {
            health.apply(&provider.id, mark, reason, now).await;
        }
        Classification::Unmarked => {
            tracing::warn!(provider = %provider.id, "probe failed without a mark; provider stays unavailable");
        }
    }
}

struct Upstream {
    status: StatusCode,
    headers: HeaderMap,
    body: reqwest::Response,
}

struct CompletionMeta {
    request_id: String,
    at: i64,
    route_id: String,
    api_key_id: String,
    model: String,
    provider_id: String,
    started: Instant,
    /// 是否生成端点；非生成端点不计 token、也不计「用量缺失」。
    generation: bool,
}

/// 响应中的模型名回写：别名 + 家族决定回写位置。
struct AliasRewrite {
    alias: String,
    family: Family,
}

struct StreamReporter {
    events: EventBus,
    meta: CompletionMeta,
    usage: Option<TokenUsage>,
    bytes_out: u64,
    done: bool,
}

impl StreamReporter {
    fn new(events: EventBus, meta: CompletionMeta) -> Self {
        Self {
            events,
            meta,
            usage: None,
            bytes_out: 0,
            done: false,
        }
    }

    fn observe_usage(&mut self, usage: TokenUsage) {
        match &mut self.usage {
            Some(existing) => existing.merge(usage),
            None => self.usage = Some(usage),
        }
    }

    fn observe_line(&mut self, line: &[u8]) {
        let Ok(text) = std::str::from_utf8(line) else {
            return;
        };
        let text = text.trim();
        let Some(data) = text.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        if let Some(usage) = extract_usage_from_sse_data(data) {
            self.observe_usage(usage);
        }
    }

    fn finish(&mut self, outcome: CompletionOutcome) {
        if self.done {
            return;
        }
        self.done = true;
        self.events.emit(CoreEvent::Completion(CompletionEvent {
            request_id: self.meta.request_id.clone(),
            at: self.meta.at,
            route_id: self.meta.route_id.clone(),
            api_key_id: self.meta.api_key_id.clone(),
            model: self.meta.model.clone(),
            provider_id: self.meta.provider_id.clone(),
            outcome,
            duration_ms: self.meta.started.elapsed().as_millis() as u64,
            bytes_out: self.bytes_out,
            usage: self.usage,
            generation: self.meta.generation,
        }));
    }
}

struct ObservedSseStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    pending: Vec<u8>,
    /// 旁路解码器：只在 `content-encoding` 非 identity 时存在，输出只喂给行解析。
    decoder: Option<BodyDecoder>,
    reporter: StreamReporter,
}

impl Stream for ObservedSseStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.reporter.bytes_out += chunk.len() as u64;
                let decoded = match self.decoder.as_mut() {
                    Some(decoder) => decoder.push(&chunk),
                    None => chunk.to_vec(),
                };
                self.pending.extend_from_slice(&decoded);
                while let Some(position) = self.pending.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = self.pending.drain(..=position).collect();
                    self.reporter.observe_line(&line);
                }
                // 单行异常超长时保留尾部，避免无限增长。
                let cap = 1024 * 1024;
                if self.pending.len() > cap {
                    let drop = self.pending.len() - cap;
                    self.pending.drain(..drop);
                }
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.reporter.finish(CompletionOutcome::UpstreamError);
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                if let Some(decoder) = self.decoder.as_mut() {
                    let tail = decoder.finish();
                    self.pending.extend_from_slice(&tail);
                }
                if !self.pending.is_empty() {
                    let tail = std::mem::take(&mut self.pending);
                    self.reporter.observe_line(&tail);
                }
                self.reporter.finish(CompletionOutcome::Completed);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for ObservedSseStream {
    fn drop(&mut self) {
        self.reporter.finish(CompletionOutcome::Aborted);
    }
}

/// 透传二进制事件流（Bedrock）：逐块转发、不缓冲、不解析 usage。
struct ObservedJsonStream {
    inner: Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>,
    reporter: StreamReporter,
}

impl Stream for ObservedJsonStream {
    type Item = Result<Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                self.reporter.bytes_out += chunk.len() as u64;
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(error))) => {
                self.reporter.finish(CompletionOutcome::UpstreamError);
                Poll::Ready(Some(Err(error)))
            }
            Poll::Ready(None) => {
                self.reporter.finish(CompletionOutcome::Completed);
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for ObservedJsonStream {
    fn drop(&mut self) {
        self.reporter.finish(CompletionOutcome::Aborted);
    }
}

/// 流式 chat 请求缺少 `stream_options.include_usage` 时补上 `true`。
///
/// 只向顶层对象末尾插入一个成员，其余字节保持原样；非流式、已声明
/// `stream_options`（客户端自选）或非 JSON 对象都原样返回。
fn ensure_stream_usage(raw: &Bytes) -> Bytes {
    const MEMBER: &str = r#""stream_options":{"include_usage":true}"#;
    let Ok(value) = serde_json::from_slice::<Value>(raw) else {
        return raw.clone();
    };
    let Some(object) = value.as_object() else {
        return raw.clone();
    };
    if object.get("stream") != Some(&Value::Bool(true)) || object.contains_key("stream_options") {
        return raw.clone();
    }
    match insert_top_level_member(raw, MEMBER) {
        Some(inserted) => Bytes::from(inserted),
        None => raw.clone(),
    }
}

/// 在顶层对象的收尾 `}` 前插入一个成员（`{}` 时不加逗号）。
fn insert_top_level_member(raw: &[u8], member: &str) -> Option<Vec<u8>> {
    let end = raw.iter().rposition(|byte| *byte == b'}')?;
    let mut before = end;
    while before > 0 && raw[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    let empty_object = raw.get(before.checked_sub(1)?) == Some(&b'{');
    let mut out = Vec::with_capacity(raw.len() + member.len() + 1);
    out.extend_from_slice(&raw[..end]);
    if !empty_object {
        out.push(b',');
    }
    out.extend_from_slice(member.as_bytes());
    out.extend_from_slice(&raw[end..]);
    Some(out)
}

/// 只改写 JSON 文本中**顶层** `key` 的字符串值，其余字节保持不变。
fn rewrite_top_level_string(raw: &[u8], key: &str, value: &str) -> Vec<u8> {
    let Some((start, end)) = top_level_string_span(raw, key) else {
        return raw.to_vec();
    };
    let escaped = json_string_content(value);
    let mut out = Vec::with_capacity(raw.len().saturating_sub(end - start) + escaped.len());
    out.extend_from_slice(&raw[..start]);
    out.extend_from_slice(escaped.as_bytes());
    out.extend_from_slice(&raw[end..]);
    out
}

/// 定位顶层 `key` 的字符串值内容区间 `[start, end)`（不含两侧引号）。
fn top_level_string_span(raw: &[u8], key: &str) -> Option<(usize, usize)> {
    let mut index = 0;
    let mut depth = 0usize;
    let mut expecting_key = false;

    while index < raw.len() {
        match raw[index] {
            b'"' => {
                let string_end = find_string_end(raw, index)?;
                if depth == 1 && expecting_key {
                    let after_key = skip_ws(raw, string_end + 1);
                    if raw.get(after_key) == Some(&b':') {
                        let value_start = skip_ws(raw, after_key + 1);
                        if &raw[index + 1..string_end] == key.as_bytes()
                            && raw.get(value_start) == Some(&b'"')
                        {
                            let content_start = value_start + 1;
                            let content_end = find_string_end(raw, value_start)?;
                            return Some((content_start, content_end));
                        }
                        expecting_key = false;
                    }
                }
                index = string_end + 1;
            }
            b'{' => {
                depth += 1;
                expecting_key = depth == 1;
                index += 1;
            }
            b'[' => {
                depth += 1;
                expecting_key = false;
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                expecting_key = false;
                index += 1;
            }
            b',' => {
                expecting_key = depth == 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

/// 定位顶层 `key` 的完整值区间；字符串返回内容区间（不含引号）。
fn top_level_value_span(raw: &[u8], key: &str) -> Option<(usize, usize)> {
    let mut index = 0;
    let mut depth = 0usize;
    let mut expecting_key = false;

    while index < raw.len() {
        match raw[index] {
            b'"' => {
                let string_end = find_string_end(raw, index)?;
                if depth == 1 && expecting_key {
                    let after_key = skip_ws(raw, string_end + 1);
                    if raw.get(after_key) == Some(&b':') {
                        let value_start = skip_ws(raw, after_key + 1);
                        if &raw[index + 1..string_end] == key.as_bytes() {
                            return value_span(raw, value_start);
                        }
                        expecting_key = false;
                    }
                }
                index = string_end + 1;
            }
            b'{' => {
                depth += 1;
                expecting_key = depth == 1;
                index += 1;
            }
            b'[' => {
                depth += 1;
                expecting_key = false;
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                expecting_key = false;
                index += 1;
            }
            b',' => {
                expecting_key = depth == 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn value_span(raw: &[u8], start: usize) -> Option<(usize, usize)> {
    match *raw.get(start)? {
        b'"' => {
            let end = find_string_end(raw, start)?;
            Some((start + 1, end))
        }
        b'{' | b'[' => {
            let end = find_container_end(raw, start)?;
            Some((start, end))
        }
        _ => {
            let mut end = start;
            while end < raw.len() && raw[end] != b',' && raw[end] != b'}' && raw[end] != b']' {
                end += 1;
            }
            while end > start && raw[end - 1].is_ascii_whitespace() {
                end -= 1;
            }
            Some((start, end))
        }
    }
}

fn find_container_end(raw: &[u8], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = start;
    while index < raw.len() {
        match raw[index] {
            b'"' => index = find_string_end(raw, index)? + 1,
            b'{' | b'[' => {
                depth += 1;
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                index += 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => index += 1,
        }
    }
    None
}

/// 按路径定位字符串值区间并替换；路径不存在或最终值不是字符串时返回 `None`。
fn rewrite_path_string(raw: &[u8], path: &[&str], value: &str) -> Option<Vec<u8>> {
    let (start, end) = find_path_span(raw, path)?;
    let escaped = json_string_content(value);
    let mut out = Vec::with_capacity(raw.len().saturating_sub(end - start) + escaped.len());
    out.extend_from_slice(&raw[..start]);
    out.extend_from_slice(escaped.as_bytes());
    out.extend_from_slice(&raw[end..]);
    Some(out)
}

fn find_path_span(raw: &[u8], path: &[&str]) -> Option<(usize, usize)> {
    let (start, end) = top_level_value_span(raw, *path.first()?)?;
    if path.len() == 1 {
        let is_string =
            raw.get(start.wrapping_sub(1)) == Some(&b'"') && raw.get(end) == Some(&b'"');
        return is_string.then_some((start, end));
    }
    let (inner_start, inner_end) = find_path_span(&raw[start..end], &path[1..])?;
    Some((start + inner_start, start + inner_end))
}

/// 回写响应中的模型名：兼容 `{"model":...}` 与 Responses API 的 `{"response":{"model":...}}`。
fn rewrite_response_model(raw: &[u8], alias: &str) -> Vec<u8> {
    match rewrite_path_string(raw, &["response", "model"], alias) {
        Some(out) => out,
        None => rewrite_top_level_string(raw, "model", alias),
    }
}

fn find_string_end(raw: &[u8], quote: usize) -> Option<usize> {
    let mut index = quote + 1;
    while index < raw.len() {
        match raw[index] {
            b'\\' => index += 2,
            b'"' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

fn skip_ws(raw: &[u8], mut index: usize) -> usize {
    while index < raw.len() && raw[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn json_string_content(value: &str) -> String {
    let quoted = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
    quoted[1..quoted.len() - 1].to_string()
}

/// 非流式响应 / 错误体中的模型名回写，按家族选择回写位置。
fn rewrite_alias_json(raw: &[u8], alias: &AliasRewrite) -> Vec<u8> {
    match alias.family {
        // Anthropic 的响应模型名在顶层 `model`。
        Family::Anthropic => rewrite_top_level_string(raw, "model", &alias.alias),
        Family::OpenAi => rewrite_response_model(raw, &alias.alias),
    }
}

/// 改写一行 SSE：只处理 `data: {...}`，其余行（含 `[DONE]`）原样返回。
fn rewrite_sse_line(line: &[u8], alias: &AliasRewrite) -> Vec<u8> {
    let content_end = line
        .iter()
        .rposition(|byte| *byte != b'\n' && *byte != b'\r')
        .map(|index| index + 1)
        .unwrap_or(0);
    let (content, ending) = line.split_at(content_end);
    let Some(rest) = content.strip_prefix(b"data:") else {
        return line.to_vec();
    };
    let payload = rest.strip_prefix(b" ").unwrap_or(rest);
    if !payload.starts_with(b"{") {
        return line.to_vec();
    }
    let rewritten = match alias.family {
        // Anthropic 流式的模型名嵌在 `message.model`；缺省回退到顶层。
        Family::Anthropic => rewrite_path_string(payload, &["message", "model"], &alias.alias)
            .unwrap_or_else(|| rewrite_top_level_string(payload, "model", &alias.alias)),
        Family::OpenAi => rewrite_response_model(payload, &alias.alias),
    };
    if rewritten == payload {
        return line.to_vec();
    }
    let mut out = Vec::with_capacity(6 + rewritten.len() + ending.len());
    out.extend_from_slice(b"data: ");
    out.extend_from_slice(&rewritten);
    out.extend_from_slice(ending);
    out
}

/// 流式改写 SSE：按行缓冲，逐行处理，不缓冲整个响应。
fn rewrite_sse_stream<S>(
    stream: S,
    alias: AliasRewrite,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static
where
    S: futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let stream = Box::pin(stream);
    futures_util::stream::unfold(
        (stream, Vec::<u8>::new(), alias, false),
        |(mut stream, mut pending, alias, mut done)| async move {
            loop {
                if let Some(position) = pending.iter().position(|byte| *byte == b'\n') {
                    let line: Vec<u8> = pending.drain(..=position).collect();
                    let out = rewrite_sse_line(&line, &alias);
                    return Some((
                        Ok::<Bytes, std::io::Error>(Bytes::from(out)),
                        (stream, pending, alias, done),
                    ));
                }
                if done {
                    if pending.is_empty() {
                        return None;
                    }
                    let tail = std::mem::take(&mut pending);
                    let out = rewrite_sse_line(&tail, &alias);
                    return Some((Ok(Bytes::from(out)), (stream, pending, alias, true)));
                }
                match stream.next().await {
                    Some(Ok(chunk)) => pending.extend_from_slice(&chunk),
                    Some(Err(error)) => {
                        done = true;
                        return Some((
                            Err(std::io::Error::other(error)),
                            (stream, pending, alias, done),
                        ));
                    }
                    None => done = true,
                }
            }
        },
    )
}

struct PrefixedTail {
    prefix: Vec<u8>,
    rest: Option<reqwest::Response>,
}

struct LastFailure {
    status: StatusCode,
    headers: HeaderMap,
    body: Option<PrefixedTail>,
}

impl LastFailure {
    fn from_body(status: StatusCode, headers: HeaderMap, body: Option<PrefixedTail>) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    fn into_response(self, alias: Option<&AliasRewrite>) -> Response {
        let LastFailure {
            status,
            headers,
            body,
        } = self;
        let (prefix, rest) = match body {
            Some(body) => (body.prefix, body.rest),
            None => (Vec::new(), None),
        };
        // 错误体通常很小，已在 prefix 里；同样按家族回写模型名。
        let prefix = match alias {
            Some(alias) => rewrite_alias_json(&prefix, alias),
            None => prefix,
        };
        let stream =
            futures_util::stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from(prefix))])
                .chain(rest_stream(rest));
        let mut response = Response::new(Body::from_stream(stream));
        *response.status_mut() = status;
        copy_response_headers(response.headers_mut(), &headers);
        response
    }
}

fn copy_response_headers(target: &mut HeaderMap, source: &HeaderMap) {
    for (name, value) in source {
        if is_dropped_header(name.as_str()) {
            continue;
        }
        target.append(name.clone(), value.clone());
    }
}

fn rest_stream(
    rest: Option<reqwest::Response>,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static {
    match rest {
        Some(response) => Either::Left(
            response
                .bytes_stream()
                .map(|chunk| chunk.map_err(std::io::Error::other)),
        ),
        None => Either::Right(futures_util::stream::empty()),
    }
}

enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R, Item> futures_util::Stream for Either<L, R>
where
    L: futures_util::Stream<Item = Item> + Unpin,
    R: futures_util::Stream<Item = Item> + Unpin,
{
    type Item = Item;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.get_mut() {
            Either::Left(inner) => std::pin::Pin::new(inner).poll_next(cx),
            Either::Right(inner) => std::pin::Pin::new(inner).poll_next(cx),
        }
    }
}

enum Attempt {
    Reached(Upstream),
    Failed {
        status: StatusCode,
        headers: HeaderMap,
        classification: Classification,
        body: Option<PrefixedTail>,
    },
    Transport {
        error: TransportError,
        classification: Classification,
    },
}

fn transport_error(error: &reqwest::Error) -> TransportError {
    let mut chain = error.to_string();
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(error);
    while let Some(inner) = source {
        chain.push_str(" <- ");
        chain.push_str(&inner.to_string());
        source = std::error::Error::source(inner);
    }
    if error.is_timeout() {
        TransportError::Timeout(chain)
    } else {
        TransportError::Other(chain)
    }
}

const DROPPED_HEADERS: [&str; 10] = [
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "content-length",
    "proxy-connection",
];

fn is_dropped_header(name: &str) -> bool {
    DROPPED_HEADERS
        .iter()
        .any(|candidate| name.eq_ignore_ascii_case(candidate))
}

enum ReadBodyError {
    TooLarge,
    Io,
}

async fn read_body_limited(body: Body, limit: usize) -> Result<Bytes, ReadBodyError> {
    let mut stream = body.into_data_stream();
    let mut collected: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ReadBodyError::Io)?;
        if collected.len() + chunk.len() > limit {
            return Err(ReadBodyError::TooLarge);
        }
        collected.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(collected))
}

/// 上游响应体的单次读取结果（统一带超时）。
enum ChunkRead {
    Data(Bytes),
    Eof,
    Io(reqwest::Error),
    Timeout,
}

/// 读取下一个响应体分片；`timeout` 是两次读取之间的最长等待。
async fn next_chunk(response: &mut reqwest::Response, timeout: Duration) -> ChunkRead {
    match tokio::time::timeout(timeout, response.chunk()).await {
        Ok(Ok(Some(chunk))) => ChunkRead::Data(chunk),
        Ok(Ok(None)) => ChunkRead::Eof,
        Ok(Err(error)) => ChunkRead::Io(error),
        Err(_) => ChunkRead::Timeout,
    }
}

/// 非流式成功响应体的读取错误。
enum ResponseBodyError {
    TooLarge,
    Timeout,
    Io,
}

/// 非流式响应体整读：单次读取超时 + 总大小上限。
async fn read_response_body_limited(
    response: &mut reqwest::Response,
    limit: usize,
    timeout: Duration,
) -> Result<Bytes, ResponseBodyError> {
    let mut collected: Vec<u8> = Vec::new();
    loop {
        match next_chunk(response, timeout).await {
            ChunkRead::Data(chunk) => {
                if collected.len() + chunk.len() > limit {
                    return Err(ResponseBodyError::TooLarge);
                }
                collected.extend_from_slice(&chunk);
            }
            ChunkRead::Eof => return Ok(Bytes::from(collected)),
            ChunkRead::Io(_) => return Err(ResponseBodyError::Io),
            ChunkRead::Timeout => return Err(ResponseBodyError::Timeout),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_reads_body_model_and_session() {
        let chat = Inbound::ChatCompletions;
        let body = br#"{"model":"gpt-5","prompt_cache_key":"s1","user":"u1"}"#;
        let payload = parse_payload(chat, body).expect("payload");
        assert_eq!(payload.model, "gpt-5");
        assert_eq!(payload.session.as_deref(), Some("s1"));

        assert!(parse_payload(chat, b"not json").is_none());
        let responses = Inbound::Responses;
        assert!(parse_payload(responses, br#"{"input":"hi"}"#).is_none());
    }

    fn vertex_provider() -> ProviderRuntime {
        let mut options = json!({
            "path_template": "/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{model}:{action}",
            "path_params": { "project": "proj", "location": "us-east5" },
        });
        ProviderRuntime {
            id: "p-vertex".to_string(),
            name: None,
            provider: "google-vertex-anthropic".to_string(),
            base_url: "https://us-east5-aiplatform.googleapis.com".to_string(),
            api_key: "{}".to_string(),
            credential_kind: "google_service_account".to_string(),
            auth_scheme: "google_service_account".to_string(),
            options: Some(options.take()),
            api_formats: vec![ApiFormat::AnthropicMessages],
            models: Vec::new(),
            enabled: true,
            priority: 1,
        }
    }

    #[test]
    fn bedrock_invoke_uses_declared_actions_and_version() {
        let mut provider = vertex_provider();
        provider.provider = "bedrock-anthropic".to_string();
        provider.base_url = "https://bedrock-runtime.us-east-1.amazonaws.com".to_string();
        provider.api_key = r#"{"access_key_id":"AKIA","secret_access_key":"s"}"#.to_string();
        provider.credential_kind = "aws_sigv4".to_string();
        provider.auth_scheme = "sigv4".to_string();
        provider.options = Some(json!({
            "path_template": "/model/{model}:{action}",
            "path_actions": { "unary": "invoke", "stream": "invoke-with-response-stream" },
            "aws_service": "bedrock",
            "anthropic_version": "bedrock-2023-05-31",
            "base_url_vars": { "region": "us-east-1" },
        }));

        // 非流式 → invoke
        let rendered = render_target(
            &provider,
            Inbound::Messages,
            Family::Anthropic,
            "/v1/messages",
            Bytes::from_static(br#"{"model":"claude","max_tokens":8}"#),
            "anthropic.claude-3-5-sonnet-20240620-v1:0",
        );
        assert_eq!(
            rendered.path.as_deref(),
            Some("/model/anthropic.claude-3-5-sonnet-20240620-v1:0:invoke")
        );
        let body: Value = serde_json::from_slice(&rendered.body).unwrap();
        assert!(body.get("model").is_none());
        assert_eq!(body["anthropic_version"], "bedrock-2023-05-31");

        // 流式 → invoke-with-response-stream
        let rendered = render_target(
            &provider,
            Inbound::Messages,
            Family::Anthropic,
            "/v1/messages",
            Bytes::from_static(br#"{"model":"claude","stream":true,"max_tokens":8}"#),
            "anthropic.claude-3-5-sonnet-20240620-v1:0",
        );
        assert_eq!(
            rendered.path.as_deref(),
            Some("/model/anthropic.claude-3-5-sonnet-20240620-v1:0:invoke-with-response-stream")
        );
    }

    #[test]
    fn vertex_anthropic_strips_model_and_injects_version() {
        let provider = vertex_provider();
        let inbound = Inbound::Messages;
        let body = br#"{"model":"claude","stream":true,"max_tokens":8}"#;
        let rendered = render_target(
            &provider,
            inbound,
            Family::Anthropic,
            "/v1/messages",
            Bytes::from_static(body),
            "claude-sonnet-4@20250514",
        );
        assert!(
            rendered
                .path
                .as_deref()
                .unwrap_or_default()
                .ends_with("/models/claude-sonnet-4@20250514:streamRawPredict"),
            "{:?}",
            rendered.path
        );
        let rewritten: Value = serde_json::from_slice(&rendered.body).unwrap();
        assert!(rewritten.get("model").is_none());
        assert_eq!(rewritten["anthropic_version"], "vertex-2023-10-16");
        assert_eq!(rewritten["stream"], true);
    }

    #[test]
    fn azure_template_keeps_api_version_query() {
        let mut provider = vertex_provider();
        provider.options = Some(json!({
            "path_template": "/openai/deployments/{deployment}/{endpoint}?api-version={api_version}",
            "path_params": { "deployment": "gpt-4o-prod", "api_version": "2024-10-21" },
        }));
        let target = Inbound::ChatCompletions;
        let rendered = render_target(
            &provider,
            target,
            Family::OpenAi,
            "/v1/chat/completions",
            Bytes::from_static(b"{}"),
            "gpt-4o",
        );
        let path = rendered.path.expect("templated path");
        assert_eq!(
            join_target("https://myres.openai.azure.com", &path, None),
            "https://myres.openai.azure.com/openai/deployments/gpt-4o-prod/chat/completions?api-version=2024-10-21"
        );
        // 入站 query 与模板 query 合并。
        assert_eq!(
            join_target("https://myres.openai.azure.com", &path, Some("a=1")),
            "https://myres.openai.azure.com/openai/deployments/gpt-4o-prod/chat/completions?api-version=2024-10-21&a=1"
        );
    }

    #[test]
    fn no_template_falls_back_to_default_path() {
        let mut provider = vertex_provider();
        provider.options = None;
        let target = Inbound::ChatCompletions;
        let rendered = render_target(
            &provider,
            target,
            Family::OpenAi,
            "/v1/chat/completions",
            Bytes::from_static(b"{}"),
            "gpt-4o",
        );
        assert!(rendered.path.is_none());
        assert_eq!(
            upstream_url(
                &provider.base_url,
                "/v1/chat/completions",
                None,
                Family::OpenAi
            ),
            "https://us-east5-aiplatform.googleapis.com/chat/completions"
        );
    }

    #[test]
    fn anthropic_session_key_uses_metadata_user_id() {
        let messages = Inbound::Messages;
        let body = br#"{"model":"claude","metadata":{"user_id":"u-42"},"max_tokens":8}"#;
        let payload = parse_payload(messages, body).expect("payload");
        assert_eq!(payload.session.as_deref(), Some("u-42"));

        // 缺 metadata 时不建立粘性，但模型名仍然可用。
        let payload = parse_payload(messages, br#"{"model":"claude","max_tokens":8}"#).unwrap();
        assert_eq!(payload.session, None);
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(hash_api_key("sw-abc"), hash_api_key("sw-abc"));
        assert_ne!(hash_api_key("sw-abc"), hash_api_key("sw-abd"));
    }

    #[test]
    fn rewrites_only_top_level_model_and_keeps_other_bytes() {
        let raw = br#"{"model":"a","nested":{"model":"b"},"arr":[{"model":"c"}],"k":1}"#;
        let out = rewrite_top_level_string(raw, "model", "z");
        assert_eq!(
            out,
            br#"{"model":"z","nested":{"model":"b"},"arr":[{"model":"c"}],"k":1}"#.to_vec()
        );
    }

    #[test]
    fn rewrites_responses_api_nested_model() {
        let raw = br#"{"type":"response.created","response":{"id":"r1","model":"up"}}"#;
        let out = rewrite_response_model(raw, "alias");
        assert_eq!(
            out,
            br#"{"type":"response.created","response":{"id":"r1","model":"alias"}}"#.to_vec()
        );
        // 非 responses 事件回退到顶层 model。
        let chunk = br#"{"model":"up","choices":[]}"#;
        assert_eq!(
            rewrite_response_model(chunk, "alias"),
            br#"{"model":"alias","choices":[]}"#.to_vec()
        );
    }

    #[test]
    fn rewrite_is_noop_without_top_level_model() {
        let raw = br#"{"nested":{"model":"b"},"input":"hi"}"#;
        assert_eq!(rewrite_top_level_string(raw, "model", "z"), raw.to_vec());
    }

    #[test]
    fn anthropic_response_model_is_top_level() {
        let alias = AliasRewrite {
            alias: "claude-alias".to_string(),
            family: Family::Anthropic,
        };
        let raw = br#"{"id":"msg_1","type":"message","model":"claude-up","content":[]}"#;
        assert_eq!(
            rewrite_alias_json(raw, &alias),
            br#"{"id":"msg_1","type":"message","model":"claude-alias","content":[]}"#.to_vec()
        );
    }

    #[test]
    fn anthropic_stream_rewrites_message_model() {
        let alias = AliasRewrite {
            alias: "claude-alias".to_string(),
            family: Family::Anthropic,
        };
        let start = b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-up\"}}\n";
        assert_eq!(
            rewrite_sse_line(start, &alias),
            b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-alias\"}}\n".to_vec()
        );
        // 其余事件、事件名行、[DONE] 原样保留。
        for line in [
            &b"event: message_start\n"[..],
            &b"data: {\"type\":\"message_delta\",\"delta\":{}}\n"[..],
            &b"data: [DONE]\n\n"[..],
            &b": keep-alive\n"[..],
        ] {
            assert_eq!(rewrite_sse_line(line, &alias), line.to_vec());
        }
    }

    #[test]
    fn rewrites_sse_data_lines_only() {
        let alias = AliasRewrite {
            alias: "alias".to_string(),
            family: Family::OpenAi,
        };
        let line = b"data: {\"model\":\"up\",\"x\":1}\n";
        assert_eq!(
            rewrite_sse_line(line, &alias),
            b"data: {\"model\":\"alias\",\"x\":1}\n".to_vec()
        );
        assert_eq!(
            rewrite_sse_line(b"data: [DONE]\n\n", &alias),
            b"data: [DONE]\n\n".to_vec()
        );
        assert_eq!(
            rewrite_sse_line(b"event: ping\n", &alias),
            b"event: ping\n".to_vec()
        );
    }

    #[test]
    fn error_bodies_follow_the_inbound_family() {
        let openai = GatewayErrorCode::InvalidApiKey.error_body(Family::OpenAi);
        assert_eq!(openai["error"]["code"], "invalid_api_key");
        assert_eq!(openai["error"]["type"], ERROR_TYPE);

        let anthropic = GatewayErrorCode::InvalidApiKey.error_body(Family::Anthropic);
        assert_eq!(anthropic["type"], "error");
        assert_eq!(anthropic["error"]["type"], "authentication_error");
        assert_eq!(anthropic["error"]["code"], "invalid_api_key");

        let anthropic_503 = GatewayErrorCode::RouteExhausted.error_body(Family::Anthropic);
        assert_eq!(anthropic_503["error"]["type"], "api_error");
    }

    #[test]
    fn stream_usage_is_injected_only_for_streaming_chat() {
        let inject = |raw: &str| {
            let body = Bytes::from(raw.to_string());
            let patched = ensure_stream_usage(&body);
            if patched == body { None } else { Some(patched) }
        };

        // 流式且未声明 `stream_options`：末尾插入，其余字节原样。
        assert_eq!(
            inject(r#"{"model":"m","stream":true}"#).unwrap(),
            Bytes::from_static(
                br#"{"model":"m","stream":true,"stream_options":{"include_usage":true}}"#
            )
        );
        // 带空白与新行：只在收尾大括号前插入第三个成员。
        assert_eq!(
            inject("{\n  \"stream\": true,\n  \"model\": \"m\"\n}\n").unwrap(),
            Bytes::from_static(
                b"{\n  \"stream\": true,\n  \"model\": \"m\"\n,\"stream_options\":{\"include_usage\":true}}\n"
            )
        );
        // 空对象形态。
        assert_eq!(
            inject(r#"{"stream":true}"#).unwrap(),
            Bytes::from_static(br#"{"stream":true,"stream_options":{"include_usage":true}}"#)
        );
        // 非流式 / 已声明 `stream_options` / 非对象 / 非法 JSON：一律不碰。
        for raw in [
            r#"{"model":"m","stream":false}"#,
            r#"{"model":"m"}"#,
            r#"{"stream":true,"stream_options":{}}"#,
            r#"[{"stream":true}]"#,
            r#"not json"#,
        ] {
            assert!(inject(raw).is_none(), "不应改写: {raw}");
        }
    }

    #[test]
    fn inbound_auth_headers_are_tried_in_order() {
        let mut headers = HeaderMap::new();
        headers.insert("x-goog-api-key", HeaderValue::from_static("k3"));
        headers.insert("x-api-key", HeaderValue::from_static("k2"));
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer k1"));
        assert_eq!(
            inbound_tokens(&headers).map(|token| token.map(str::to_string)),
            [Some("k1".to_string()), Some("k2".to_string())]
        );

        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("k1"));
        assert_eq!(inbound_tokens(&headers)[0], None);
    }
}
