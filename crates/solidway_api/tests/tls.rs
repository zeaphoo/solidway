//! 内置 TLS 的进程内集成测试：双监听、缓存证书热装载、跳转、等待签发、HSTS。
//!
//! 不连真实 ACME：证书由 rcgen 生成后直接写入 `data_root/tls` 的缓存位置，
//! 与生产路径（启动时装载缓存）一致。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use solidway_api::settings::{self, TlsChallenge, TlsSettings};
use solidway_api::tls::{self, CertState, TlsRuntime};
use solidway_api::{Api, ApiConfig, RunMode};

const DOMAIN: &str = "gw.example.com";

fn temp_root(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "solidway-tls-it-{tag}-{}",
        uuid::Uuid::new_v4()
    ))
}

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    listener.local_addr().expect("addr").port()
}

/// 生成自签名证书并写入缓存目录（模拟「已有缓存证书」的启动路径）。
fn write_cached_cert(data_root: &PathBuf, days: i64) {
    let store = tls::store::CertStore::new(data_root);
    let key = rcgen::KeyPair::generate().expect("key");
    let params = rcgen::CertificateParams::new(vec![DOMAIN.to_string()]).expect("params");
    let cert = params.self_signed(&key).expect("cert");
    let now = solidway_api::state::now_unix();
    store
        .save(&tls::stored_cert(
            &[DOMAIN.to_string()],
            &cert.pem(),
            &key.serialize_pem(),
            now + days * 24 * 3600,
        ))
        .expect("save cached cert");
}

struct Harness {
    runtime: Arc<TlsRuntime>,
    api: Api,
    _root: PathBuf,
}

async fn start(tag: &str, hsts: bool, with_cert: bool) -> Harness {
    let root = temp_root(tag);
    std::fs::create_dir_all(&root).expect("temp root");
    let api = Api::init(ApiConfig {
        data_root: root.clone(),
        master_key: None,
        mode: RunMode::Standalone,
        web_assets: None,
    })
    .await
    .expect("init api");

    let admin_id: String =
        sqlx::query_scalar("SELECT id FROM users WHERE role = 'admin' LIMIT 1")
            .fetch_one(&api.state().pool)
            .await
            .expect("admin id");

    let access = settings::AccessSettings {
        public_origins: vec![format!("https://{DOMAIN}")],
    };
    settings::save_access(&api.state().pool, &access, &admin_id, 1)
        .await
        .expect("save access");

    if with_cert {
        write_cached_cert(&root, 60);
        }

    let tls_settings = TlsSettings {
        enabled: true,
        email: "ops@example.com".to_string(),
        challenge: TlsChallenge::Http01,
        directory: settings::AcmeDirectory::Staging,
        http_port: free_port(),
        https_port: free_port(),
        hsts,
    };
    // 让数据库里的设置与运行中的监听一致（status 的 restart_required 依赖它）。
    settings::save_tls(&api.state().pool, &tls_settings, &admin_id, 1)
        .await
        .expect("save tls");
    api.state().reload_settings().await.expect("reload");

    let app = Router::new()
        .route("/", get(|| async { "ok" }))
        .route("/foo", get(|| async { "foo" }));
    let runtime = tls::start(api.state(), tls_settings, app)
        .await
        .expect("start tls");

    Harness {
        runtime,
        api,
        _root: root,
    }
}

fn client(domain: &str, port: u16, accept_invalid: bool) -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(accept_invalid)
        .resolve(domain, SocketAddr::from(([127, 0, 0, 1], port)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("client")
}

#[tokio::test]
async fn https_serves_cached_cert() {
    let harness = start("https", false, true).await;
    let port = harness.runtime.https_addr().port();
    let resp = client(DOMAIN, port, true)
        .get(format!("https://{DOMAIN}/"))
        .send()
        .await
        .expect("https request");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn http_redirects_to_https_and_keeps_path_and_query() {
    let harness = start("redirect", false, true).await;
    let http_port = harness.runtime.http_addr().port();
    let https_port = harness.runtime.https_addr().port();

    let resp = client(DOMAIN, http_port, true)
        .get(format!("http://{DOMAIN}:{http_port}/foo?a=1"))
        .send()
        .await
        .expect("http request");
    assert_eq!(resp.status(), 308);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, format!("https://{DOMAIN}:{https_port}/foo?a=1"));
}

#[tokio::test]
async fn acme_challenge_path_is_served_before_redirect() {
    let harness = start("challenge", false, true).await;
    let http_port = harness.runtime.http_addr().port();
    harness
        .runtime
        .challenges()
        .insert("tok-1", "tok-1.thumbprint");

    let resp = client(DOMAIN, http_port, true)
        .get(format!("http://{DOMAIN}:{http_port}/.well-known/acme-challenge/tok-1"))
        .send()
        .await
        .expect("challenge request");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "tok-1.thumbprint");

    // 未知 token 不跳转。
    let resp = client(DOMAIN, http_port, true)
        .get(format!("http://{DOMAIN}:{http_port}/.well-known/acme-challenge/nope"))
        .send()
        .await
        .expect("challenge request");
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn without_cert_http_returns_503_and_never_redirects() {
    let harness = start("waiting", false, false).await;
    let http_port = harness.runtime.http_addr().port();

    let resp = client(DOMAIN, http_port, true)
        .get(format!("http://{DOMAIN}:{http_port}/foo"))
        .send()
        .await
        .expect("http request");
    assert_eq!(resp.status(), 503);
    assert!(resp.headers().get("location").is_none());

    let status = tls::status_for(harness.api.state()).await.expect("status");
    assert_eq!(status.cert.state, CertState::Waiting.as_str());
    assert!(status.cert.not_after.is_none());
    assert_eq!(status.domains, vec![DOMAIN.to_string()]);
    assert!(status.skipped.is_empty());
    assert!(!status.restart_required);
}

#[tokio::test]
async fn unknown_host_is_not_redirected() {
    let harness = start("unknown", false, true).await;
    let http_port = harness.runtime.http_addr().port();

    let resp = client("other.example.com", http_port, true)
        .get(format!("http://other.example.com:{http_port}/foo"))
        .send()
        .await
        .expect("http request");
    assert_eq!(resp.status(), 421);
}

#[tokio::test]
async fn hsts_header_only_when_enabled() {
    let enabled = start("hsts-on", true, true).await;
    let port = enabled.runtime.https_addr().port();
    let resp = client(DOMAIN, port, true)
        .get(format!("https://{DOMAIN}/"))
        .send()
        .await
        .expect("https request");
    assert_eq!(
        resp.headers().get("strict-transport-security").unwrap(),
        "max-age=86400"
    );

    let disabled = start("hsts-off", false, true).await;
    let port = disabled.runtime.https_addr().port();
    let resp = client(DOMAIN, port, true)
        .get(format!("https://{DOMAIN}/"))
        .send()
        .await
        .expect("https request");
    assert!(resp.headers().get("strict-transport-security").is_none());
}

#[tokio::test]
async fn port_hint_reports_mapping_need() {
    let harness = start("hint", false, true).await;
    let status = tls::status_for(harness.api.state()).await.expect("status");
    let hint = status.challenge_port_hint.expect("hint");
    assert!(hint.contains("80"), "{hint}");
}
