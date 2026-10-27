//! 内置 TLS / ACME：双监听（HTTPS 与 ACME + 跳转）、证书缓存、热替换、状态与人工重试。
//!
//! 决策与选型见 `docs/adr/0002-tls-acme-library.md`；行为以 `openspec` 的
//! `builtin-tls-acme` 为准。

pub mod acme;
pub mod challenge;
pub mod domain;
pub mod resolver;
pub mod scheduler;
pub mod store;

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use tokio::sync::Notify;

use crate::error::{ApiError, Result};
use crate::settings::{TlsSettings, load_tls};
use crate::state::{AppState, now_unix};
use crate::tls::challenge::ChallengeStore;
use crate::tls::domain::{DerivedDomains, SkippedOrigin, derive_domains};
use crate::tls::resolver::CertResolver;
use crate::tls::store::{CertStore, StoredCert};

pub use domain::derive_domains as derive;

/// 证书状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertState {
    /// 证书可用。
    Ok,
    /// 尚未签发成功（首次签发中或失败重试中）。
    Waiting,
    /// 连续失败，已暂停自动重试。
    Paused,
}

impl CertState {
    pub fn as_str(self) -> &'static str {
        match self {
            CertState::Ok => "ok",
            CertState::Waiting => "waiting",
            CertState::Paused => "paused",
        }
    }
}

#[derive(Debug, Default)]
struct StatusInner {
    state: CertState,
    not_after: Option<i64>,
    last_renewed_at: Option<i64>,
    last_error: Option<String>,
    failed_attempts: u32,
    window_start: i64,
}

impl Default for CertState {
    fn default() -> Self {
        CertState::Waiting
    }
}

/// `GET /api/tls/status` 的响应。
#[derive(Debug, Clone, Serialize)]
pub struct TlsStatus {
    pub enabled: bool,
    pub challenge: &'static str,
    pub directory: &'static str,
    pub domains: Vec<String>,
    pub skipped: Vec<SkippedOrigin>,
    pub challenge_port_hint: Option<String>,
    pub cert: CertStatus,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CertStatus {
    pub state: &'static str,
    pub not_after: Option<i64>,
    pub last_renewed_at: Option<i64>,
    pub last_error: Option<String>,
    pub failed_attempts_24h: u32,
}

/// 运行中的内置 TLS。
pub struct TlsRuntime {
    settings: TlsSettings,
    domains: Vec<String>,
    skipped: Vec<SkippedOrigin>,
    store: Arc<CertStore>,
    resolver: Arc<CertResolver>,
    challenges: Arc<ChallengeStore>,
    status: Mutex<StatusInner>,
    retry: Notify,
    /// 实际绑定的监听地址（端口可能由系统分配，供日志与测试）。
    http_addr: SocketAddr,
    https_addr: SocketAddr,
}

impl std::fmt::Debug for TlsRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TlsRuntime")
            .field("domains", &self.domains)
            .finish_non_exhaustive()
    }
}

impl TlsRuntime {
    pub fn domains(&self) -> &[String] {
        &self.domains
    }

    /// HTTP（ACME / 跳转）实际监听地址。
    pub fn http_addr(&self) -> SocketAddr {
        self.http_addr
    }

    /// HTTPS 实际监听地址。
    pub fn https_addr(&self) -> SocketAddr {
        self.https_addr
    }

    pub fn store(&self) -> &CertStore {
        &self.store
    }

    pub fn resolver(&self) -> &Arc<CertResolver> {
        &self.resolver
    }

    pub fn challenges(&self) -> &ChallengeStore {
        &self.challenges
    }

    pub fn settings(&self) -> &TlsSettings {
        &self.settings
    }

    pub fn skipped(&self) -> &[SkippedOrigin] {
        &self.skipped
    }

    pub fn is_paused(&self) -> bool {
        self.status.lock().expect("tls status").state == CertState::Paused
    }

    fn next_attempt_delay(&self) -> std::time::Duration {
        let status = self.status.lock().expect("tls status");
        scheduler::next_delay(now_unix(), status.not_after, status.failed_attempts)
    }

    async fn wait_for_retry(&self) {
        self.retry.notified().await;
    }

    /// 人工重试入口（`POST /api/tls/retry`）。
    pub fn request_retry(&self) {
        self.retry.notify_one();
    }

    fn reset_failures(&self) {
        let mut status = self.status.lock().expect("tls status");
        status.failed_attempts = 0;
        status.window_start = now_unix();
        if status.state == CertState::Paused {
            status.state = if self.resolver.has_cert() {
                CertState::Ok
            } else {
                CertState::Waiting
            };
        }
    }

    fn record_success(&self, not_after: i64) {
        let mut status = self.status.lock().expect("tls status");
        status.state = CertState::Ok;
        status.not_after = Some(not_after);
        status.last_renewed_at = Some(now_unix());
        status.last_error = None;
        status.failed_attempts = 0;
        status.window_start = now_unix();
    }

    fn record_failure(&self, error: String) {
        let now = now_unix();
        let mut status = self.status.lock().expect("tls status");
        if status.window_start == 0 || now - status.window_start > scheduler::FAILURE_WINDOW_SECS {
            status.window_start = now;
            status.failed_attempts = 0;
        }
        status.failed_attempts += 1;
        status.last_error = Some(error);
        status.state = if status.failed_attempts >= scheduler::MAX_FAILURES_PER_WINDOW {
            CertState::Paused
        } else {
            CertState::Waiting
        };
    }

    fn snapshot(&self) -> CertStatus {
        let status = self.status.lock().expect("tls status");
        CertStatus {
            state: status.state.as_str(),
            not_after: status.not_after,
            last_renewed_at: status.last_renewed_at,
            last_error: status.last_error.clone(),
            failed_attempts_24h: status.failed_attempts,
        }
    }

    /// 对外状态（含 `restart_required` 判定需要的存储设置）。
    pub fn status(&self, stored: &TlsSettings) -> TlsStatus {
        TlsStatus {
            enabled: true,
            challenge: self.settings.challenge.as_str(),
            directory: if self.settings.directory == crate::settings::AcmeDirectory::Staging {
                "staging"
            } else {
                "production"
            },
            domains: self.domains.clone(),
            skipped: self.skipped.clone(),
            challenge_port_hint: self.settings.port_hint(),
            cert: self.snapshot(),
            restart_required: self.settings.enabled != stored.enabled
                || self.settings.http_port != stored.http_port
                || self.settings.https_port != stored.https_port
                || self.settings.challenge != stored.challenge
                || self.settings.directory != stored.directory,
        }
    }
}

/// 启动内置 TLS 监听：装配证书缓存 → 绑定双监听 → 起服务循环。
///
/// 续期任务不在本函数内启动（见 [`spawn_renewal`]），这样测试可以只验证监听行为。
pub async fn start(
    state: &AppState,
    settings: TlsSettings,
    app: Router,
) -> Result<Arc<TlsRuntime>> {
    let DerivedDomains { domains, skipped } =
        derive_domains(&state.settings().access.public_origins);
    settings.validate(&domains)?;

    // TLS 只对"对外"地址生效：跳过回环（127.0.0.1:443 没意义，还可能与本地开发冲突）；
    // 若配置里只有回环地址（开发机），回退为按配置监听。
    let configured: Vec<IpAddr> = state.settings().network.bind_ips();
    let mut bind_ips: Vec<IpAddr> = configured
        .iter()
        .copied()
        .filter(|ip| !ip.is_loopback())
        .collect();
    if bind_ips.is_empty() {
        bind_ips = configured;
    }
    if bind_ips.is_empty() {
        return Err(ApiError::InvalidRequest(
            "没有可用的绑定地址（network.bind_addrs 为空）".to_string(),
        ));
    }

    // 先绑定：任何一个端口 / 地址失败都必须上报，不静默降级到明文。
    let mut https_listeners = Vec::with_capacity(bind_ips.len());
    let mut http_listeners = Vec::with_capacity(bind_ips.len());
    let mut first: Option<(SocketAddr, SocketAddr)> = None;
    for ip in &bind_ips {
        let https_listener = bind(SocketAddr::new(*ip, settings.https_port), "https").await?;
        let http_listener = bind(SocketAddr::new(*ip, settings.http_port), "http").await?;
        let fallback = SocketAddr::new(*ip, 0);
        let https_addr = https_listener.local_addr().unwrap_or(fallback);
        let http_addr = http_listener.local_addr().unwrap_or(fallback);
        first.get_or_insert((https_addr, http_addr));
        https_listeners.push(https_listener);
        http_listeners.push(http_listener);
    }
    let (https_addr, http_addr) = first.unwrap_or_else(|| {
        let fallback = SocketAddr::new(bind_ips[0], 0);
        (fallback, fallback)
    });

    let store = Arc::new(CertStore::new(&state.data_root));
    let resolver = Arc::new(CertResolver::new());
    let challenges = Arc::new(ChallengeStore::default());

    // 先用缓存证书起服务；无缓存时 HTTPS 握手失败，HTTP 侧进入「等待签发」。
    let now = now_unix();
    let mut inner = StatusInner::default();
    let cached = match store.load_matching_async(&domains, now).await {
        Some(cert) => Some(cert),
        None => store.load_latest_valid_async(now).await,
    };
    if let Some(cert) = cached {
        // 域名集合变了：旧证书先服务，但必须立刻重新签发（按 30 天窗口会等到过期）。
        let domains_changed = cert.domains != domains;
        if domains_changed {
            tracing::info!(
                cached = ?cert.domains,
                current = ?domains,
                "certificate domain set changed; re-issuing now"
            );
        }
        match resolver.set_cert(&cert.chain_pem, &cert.key_pem) {
            Ok(()) => {
                inner.state = CertState::Ok;
                // `None` 表示「待签发」：调度器会在下一个 tick 立刻尝试。
                inner.not_after = (!domains_changed).then_some(cert.not_after);
                inner.last_renewed_at = Some(cert.last_renewed_at);
                tracing::info!(
                    domains = ?cert.domains,
                    not_after = cert.not_after,
                    "cached certificate loaded"
                );
            }
            Err(error) => tracing::warn!(%error, "cached certificate could not be loaded; re-issuing"),
        }
    }

    let runtime = Arc::new(TlsRuntime {
        settings: settings.clone(),
        domains,
        skipped,
        store,
        resolver,
        challenges,
        status: Mutex::new(inner),
        retry: Notify::new(),
        http_addr,
        https_addr,
    });

    // 注册到应用状态，供 `GET /api/tls/status` 与 `POST /api/tls/retry` 使用。
    *state.tls.write().expect("tls lock") = Some(Arc::clone(&runtime));

    for (https_listener, http_listener) in
        https_listeners.into_iter().zip(http_listeners.into_iter())
    {
        spawn_https(
            Arc::clone(&runtime),
            https_listener,
            app.clone(),
            settings.hsts,
        );
        spawn_http(Arc::clone(&runtime), http_listener, app.clone());
    }

    Ok(runtime)
}

/// 起续期调度任务。
///
/// 与 [`start`] 分开是为了让「监听」与「签发」互不耦合：测试可以只验证监听行为，
/// 生产由二进制在装配成功后显式开启。
pub fn spawn_renewal(runtime: Arc<TlsRuntime>) {
    tokio::spawn(async move { scheduler::run(runtime).await });
}

async fn bind(addr: SocketAddr, kind: &str) -> Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(addr).await.map_err(|error| {
        crate::events::bind_failed(
            &addr.to_string(),
            crate::events::bind_error_kind(&error),
            None,
        );
        ApiError::internal(format!("{kind} 监听 {addr} 失败: {error}"))
    })
}

/// HTTPS 接受循环：rustls 终止 TLS（h2 / http/1.1），服务同一套 router。
fn spawn_https(runtime: Arc<TlsRuntime>, listener: tokio::net::TcpListener, app: Router, hsts: bool) {
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use hyper_util::server::conn::auto::Builder;

    let acceptor = tokio_rustls::TlsAcceptor::from(runtime.resolver().server_config());
    let app = if hsts {
        // HSTS 只在显式开启时发送；值取保守的短 max-age。
        app.layer(tower_http::set_header::SetResponseHeaderLayer::if_not_present(
            header::STRICT_TRANSPORT_SECURITY,
            axum::http::HeaderValue::from_static("max-age=86400"),
        ))
    } else {
        app
    };

    tokio::spawn(async move {
        loop {
            let Ok((stream, peer)) = listener.accept().await else {
                continue;
            };
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                let Ok(tls) = acceptor.accept(stream).await else {
                    // 无证书时的握手失败属预期（等待签发）。
                    return;
                };
                let service =
                    hyper_util::service::TowerToHyperService::new(app);
                let builder = Builder::new(TokioExecutor::new());
                if let Err(error) = builder
                    .serve_connection_with_upgrades(TokioIo::new(tls), service)
                    .await
                {
                    tracing::debug!(%peer, %error, "HTTPS connection closed");
                }
            });
        }
    });
}

/// HTTP 监听：ACME 挑战优先，其余按「跳转 / 等待签发」处理。
fn spawn_http(runtime: Arc<TlsRuntime>, listener: tokio::net::TcpListener, app: Router) {
    let router = Router::new()
        .route(
            "/.well-known/acme-challenge/{token}",
            get(http01_challenge),
        )
        .fallback(plain_http_fallback)
        // 应用本身只经 HTTPS 暴露；未知路径交给 fallback 决定跳转或 503。
        .with_state((Arc::clone(&runtime), app));
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, router).await {
            tracing::error!(%error, "HTTP (ACME/redirect) listener exited unexpectedly");
        }
    });
}

type HttpState = (Arc<TlsRuntime>, Router);

async fn http01_challenge(
    State((runtime, _app)): State<HttpState>,
    Path(token): Path<String>,
) -> Response {
    match runtime.challenges().get(&token) {
        Some(value) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/plain")],
            value,
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "challenge not found").into_response(),
    }
}

async fn plain_http_fallback(
    State((runtime, _app)): State<HttpState>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(|host| host.split(':').next().unwrap_or(host).to_ascii_lowercase())
    else {
        return (StatusCode::MISDIRECTED_REQUEST, "unknown host").into_response();
    };
    if !runtime.domains().iter().any(|domain| domain == &host) {
        // 未知 Host 不跳转，避免开放重定向。
        return (StatusCode::MISDIRECTED_REQUEST, "unknown host").into_response();
    }
    if !runtime.resolver().has_cert() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "HTTPS 证书尚未签发完成，请稍后重试",
        )
            .into_response();
    }
    let port = runtime.settings().https_port;
    let authority = if port == 443 {
        host
    } else {
        format!("{host}:{port}")
    };
    let target = match uri.query() {
        Some(query) => format!("https://{authority}{}?{query}", uri.path()),
        None => format!("https://{authority}{}", uri.path()),
    };
    (
        StatusCode::PERMANENT_REDIRECT,
        [(header::LOCATION, target)],
    )
        .into_response()
}

/// 状态接口的统一构造：TLS 未启用时返回 `enabled=false` 与空证书字段。
pub async fn status_for(state: &AppState) -> Result<TlsStatus> {
    let stored = load_tls(&state.pool).await?;
    let runtime = state.tls();
    if let Some(runtime) = runtime {
        return Ok(runtime.status(&stored));
    }
    let DerivedDomains { domains, skipped } =
        derive_domains(&state.settings().access.public_origins);
    Ok(TlsStatus {
        enabled: false,
        challenge: stored.challenge.as_str(),
        directory: if stored.directory == crate::settings::AcmeDirectory::Staging {
            "staging"
        } else {
            "production"
        },
        domains,
        skipped,
        challenge_port_hint: stored.port_hint(),
        cert: CertStatus {
            state: CertState::Waiting.as_str(),
            not_after: None,
            last_renewed_at: None,
            last_error: None,
            failed_attempts_24h: 0,
        },
        // 数据库要求启用但本进程没有运行 TLS 监听：需要重启生效。
        restart_required: stored.enabled,
    })
}

/// 供测试构造缓存证书。
pub fn stored_cert(domains: &[String], chain_pem: &str, key_pem: &str, not_after: i64) -> StoredCert {
    StoredCert {
        domains: domains.to_vec(),
        chain_pem: chain_pem.to_string(),
        key_pem: key_pem.to_string(),
        not_after,
        last_renewed_at: now_unix(),
    }
}
