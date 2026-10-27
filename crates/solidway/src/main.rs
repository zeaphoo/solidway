use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use solidway_api::events;
use solidway_api::net_holder;
use solidway_api::settings::NetworkSettings;
use solidway_api::{Api, ApiConfig, ListenInfo, ListenSource, RunMode};

const USAGE: &str = "\
用法: solidway <data_root> [选项]

选项:
  --mode <desktop|standalone>  运行模式（默认 standalone）
                               desktop: 允许由托管方重启，并启用 stdin 存活看门狗
  --listen <addr>              覆盖监听地址（host:port、:port 或端口号）
  --master-key <hex|base64>    直接提供主密钥（32 字节；仅用于临时恢复）
  --web-assets <dir>           使用磁盘上的前端产物目录（覆盖内置 / 默认）
  --reset-password             将 admin 重置为未设置口令（不启动服务，随后重启即可设置新口令）
  -h, --help                   显示本帮助

说明:
  - 所有可调参数来自数据库（data_root/solidway.db），缺失时使用内置默认值。
  - 端口 0 不被接受；端口必须由设置显式给出。
  - desktop 模式下 stdin 是存活信号：托管方退出（stdin 关闭）即退出。
  - 首次启动会输出一次性引导令牌（`solidway: bootstrap ...`），用它打开
    /bootstrap#<token> 设置 admin 口令；令牌不写入日志。
";

struct Cli {
    data_root: PathBuf,
    mode: RunMode,
    listen: Option<String>,
    master_key: Option<String>,
    web_assets: Option<PathBuf>,
    reset_password: bool,
}

fn parse_args() -> Result<Option<Cli>, String> {
    let mut args = std::env::args().skip(1);
    let mut data_root: Option<PathBuf> = None;
    let mut mode = RunMode::Standalone;
    let mut listen = None;
    let mut master_key = None;
    let mut web_assets = None;
    let mut reset_password = false;

    while let Some(arg) = args.next() {
        let mut take = |name: &str| -> Result<String, String> {
            args.next()
                .ok_or_else(|| format!("{name} 需要一个值"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--mode" => {
                mode = match take("--mode")?.as_str() {
                    "desktop" => RunMode::Desktop,
                    "standalone" => RunMode::Standalone,
                    other => return Err(format!("--mode 非法: {other}（应为 desktop 或 standalone）")),
                };
            }
            "--listen" => listen = Some(take("--listen")?),
            "--master-key" => master_key = Some(take("--master-key")?),
            "--web-assets" => web_assets = Some(PathBuf::from(take("--web-assets")?)),
            "--reset-password" => reset_password = true,
            other if other.starts_with('-') => return Err(format!("未知选项: {other}")),
            other => {
                if data_root.is_some() {
                    return Err(format!("多余的参数: {other}"));
                }
                data_root = Some(PathBuf::from(other));
            }
        }
    }

    let data_root = data_root.ok_or_else(|| "缺少 <data_root>".to_string())?;
    Ok(Some(Cli {
        data_root,
        mode,
        listen,
        master_key,
        web_assets,
        reset_password,
    }))
}

/// 解析 `--listen`：`host:port` / `:port` / 裸 `port`（后两者沿用数据库的 bind_addr）。
fn parse_listen(raw: &str, fallback_bind: &str) -> Result<std::net::SocketAddr, String> {
    let reject_zero = |port: u16| -> Result<u16, String> {
        if port == 0 {
            Err("--listen 不接受端口 0（端口必须显式确定）".to_string())
        } else {
            Ok(port)
        }
    };

    if let Ok(addr) = raw.trim().parse::<std::net::SocketAddr>() {
        reject_zero(addr.port())?;
        return Ok(addr);
    }

    let port_text = raw.trim().trim_start_matches(':');
    let port = port_text
        .parse::<u16>()
        .map_err(|_| format!("--listen 无法解析: {raw}（应为 host:port、:port 或端口号）"))?;
    let port = reject_zero(port)?;
    let bind = fallback_bind
        .parse::<std::net::IpAddr>()
        .map_err(|_| format!("数据库中的 bind_addr 非法: {fallback_bind}"))?;
    Ok(std::net::SocketAddr::new(bind, port))
}

fn desired_addrs(network: &NetworkSettings) -> Result<Vec<std::net::SocketAddr>, String> {
    let mut addrs = Vec::new();
    for raw in &network.bind_addrs {
        let ip = raw
            .parse::<std::net::IpAddr>()
            .map_err(|_| format!("数据库中的绑定地址非法: {raw}"))?;
        addrs.push(std::net::SocketAddr::new(ip, network.port));
    }
    if addrs.is_empty() {
        return Err("数据库中的绑定地址为空".to_string());
    }
    Ok(addrs)
}

/// 内部监听的 loopback 化：端口不变，IP 换成回环地址。
fn loopback_addr(addr: std::net::SocketAddr) -> std::net::SocketAddr {
    if addr.ip().is_loopback() {
        return addr;
    }
    let ip = match addr {
        std::net::SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
        std::net::SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
    };
    std::net::SocketAddr::new(ip, addr.port())
}

/// 桌面自动登录的基地址：只走本机回环，MUST NOT 经公网 / TLS 域名。
///
/// 令牌只在壳与本地 webview 之间传递；即使 desktop 模式同时对外服务，
/// 也不能让 admin 之外的入口碰到它。TLS 模式下内部监听本身已是 loopback。
fn local_base(effective: &str) -> String {
    let addr: std::net::SocketAddr = match effective.parse() {
        Ok(addr) => addr,
        // 解析失败时原样返回（`effective` 只来自本地 bind 结果，正常不会走到）。
        Err(_) => return format!("http://{effective}"),
    };
    if addr.ip().is_unspecified() {
        let ip = if addr.is_ipv6() { "[::1]" } else { "127.0.0.1" };
        format!("http://{ip}:{}", addr.port())
    } else {
        format!("http://{addr}")
    }
}

/// 引导链接的对外基地址：启用内置 TLS 且有合法域名时走 https。
fn bootstrap_base(api: &Api, effective: &str) -> String {
    let settings = api.state().settings();
    if settings.tls.enabled {
        let derived =
            solidway_api::tls::domain::derive_domains(&settings.access.public_origins);
        if let Some(domain) = derived.domains.first() {
            return if settings.tls.https_port == 443 {
                format!("https://{domain}")
            } else {
                format!("https://{domain}:{}", settings.tls.https_port)
            };
        }
    }
    format!("http://{effective}")
}

/// desktop 模式：stdin 是托管方的存活信号，读到 EOF 触发收尾后退出。
///
/// 走与信号一致的收尾流程（有超时上限），避免"托管方崩溃"时丢失用量增量。
fn spawn_stdin_watchdog(shutdown: ShutdownTx) {
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = [0u8; 64];
        loop {
            match std::io::stdin().read(&mut buf) {
                Ok(0) => {
                    tracing::warn!("stdin closed (host exited); shutting down");
                    let _ = shutdown.send(true);
                    return;
                }
                Ok(_) => continue,
                Err(_) => {
                    let _ = shutdown.send(true);
                    return;
                }
            }
        }
    });
}

/// 收尾预算：等在途请求的上限与总预算。
const INFLIGHT_GRACE: Duration = Duration::from_secs(2);
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(5);

type ShutdownTx = tokio::sync::watch::Sender<bool>;

fn trigger(tx: &ShutdownTx) {
    let _ = tx.send(true);
}

/// 监听 SIGTERM / SIGINT（Ctrl-C）。
fn spawn_signal_listener(shutdown: ShutdownTx) {
    tokio::spawn(async move {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let mut term = match signal(SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(error) => {
                    tracing::warn!(%error, "failed to register SIGTERM handler");
                    return;
                }
            };
            tokio::select! {
                _ = term.recv() => tracing::info!("received SIGTERM; shutting down"),
                _ = tokio::signal::ctrl_c() => tracing::info!("received SIGINT; shutting down"),
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            tracing::info!("received Ctrl-C; shutting down");
        }
        trigger(&shutdown);
    });
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cli = match parse_args() {
        Ok(Some(cli)) => cli,
        Ok(None) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    // 口令重置不走服务启动流程：只改库，不加载主密钥、不占端口。
    if cli.reset_password {
        return match solidway_api::reset_admin_password(&cli.data_root).await {
            Ok(()) => {
                println!(
                    "已将 admin 重置为「未设置口令」。\n\
                     下一步：启动 solidway（或桌面应用），按提示打开一次性链接设置新口令。\n\
                     data_root: {}",
                    cli.data_root.display()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("重置失败: {error}");
                ExitCode::FAILURE
            }
        };
    }

    let api = match Api::init(ApiConfig {
        data_root: cli.data_root.clone(),
        master_key: cli.master_key.clone(),
        mode: cli.mode,
        web_assets: cli.web_assets.clone(),
    })
    .await
    {
        Ok(api) => api,
        Err(err) => {
            eprintln!("初始化失败: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 监听地址：命令行优先，否则用数据库里的 network 设置。
    let network = api.state().settings().network.clone();
    // `--listen` 只覆盖主监听且为单地址；否则按设置里的地址列表。
    let (desired, source) = match cli.listen.as_deref() {
        Some(raw) => (
            vec![match parse_listen(raw, &network.bind_addr) {
                Ok(addr) => addr,
                Err(error) => {
                    eprintln!("{error}");
                    return ExitCode::from(2);
                }
            }],
            ListenSource::CommandLine,
        ),
        None => match desired_addrs(&network) {
            Ok(addrs) => (addrs, ListenSource::Database),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        },
    };

    // 内置 TLS 启用时，内部监听（管理面 + 数据面）只绑 loopback：
    // 公网入口只剩 https_port 与 http_port（ACME / 跳转）。
    let tls_settings = api.state().settings().tls.clone();
    let internal_addrs: Vec<std::net::SocketAddr> = if tls_settings.enabled {
        desired.iter().map(|addr| loopback_addr(*addr)).collect()
    } else {
        desired.clone()
    };

    // 逐个地址绑定：任一失败即整体失败（不启动半套服务）。
    let mut listeners = Vec::with_capacity(internal_addrs.len());
    let mut effective: Vec<String> = Vec::with_capacity(internal_addrs.len());
    for addr in &internal_addrs {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                let bound = listener.local_addr().unwrap_or(*addr);
                events::listening(&bound.to_string());
                effective.push(bound.to_string());
                listeners.push(listener);
            }
            Err(err) => {
                let holder = net_holder::find(addr.port());
                events::bind_failed(
                    &addr.to_string(),
                    events::bind_error_kind(&err),
                    holder.as_ref().map(|(holder, more)| (holder, *more)),
                );
                // 单次写：与 stdout 事件行共用日志文件时避免交错。
                eprint!("{}", format!("监听 {addr} 失败: {err}\n"));
                return ExitCode::FAILURE;
            }
        }
    }
    // 引导令牌：只经 stdout 事件交付（壳不写日志；standalone 由终端直接看到）。
    if let Some(token) = api.bootstrap_token() {
        let primary = effective.first().cloned().unwrap_or_else(|| "-".to_string());
        let url = solidway_api::bootstrap_url(&bootstrap_base(&api, &primary), token);
        events::bootstrap(token, &url);
    }
    // 桌面自动登录：仅 desktop 模式有令牌；行内含令牌，壳 MUST NOT 写日志。
    // 基址固定用本机回环：token 不经过公网 / TLS 域名。
    if let Some(token) = api.desktop_login_token() {
        let primary = effective.first().cloned().unwrap_or_else(|| "-".to_string());
        let url = format!("{}/desktop#{}", local_base(&primary), token);
        events::desktop_login(&url);
    }
    api.set_listen(ListenInfo {
        effective: effective.clone(),
        source,
    });

    // 装配数据面引擎：读 DB → CoreConfig → Core，并启动事件消费。
    let core = match api.build_core().await {
        Ok(core) => core,
        Err(err) => {
            eprintln!("数据面引擎装配失败: {err}");
            return ExitCode::FAILURE;
        }
    };

    // 关闭信号：SIGTERM / SIGINT；桌面模式额外把 stdin EOF 当作同一信号。
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    // 管理面 `/api/app/restart` 与信号走同一关闭通道（停接新连接 + 等在途）。
    api.state().install_shutdown(shutdown_tx.clone());
    spawn_signal_listener(shutdown_tx.clone());
    if cli.mode.is_desktop() {
        spawn_stdin_watchdog(shutdown_tx.clone());
    }

    tracing::info!(addrs = ?effective, mode = cli.mode.as_str(), "solidway started");

    // 管理面（/api）+ 数据面（/health、/v1/*）同端口；其余路径走静态托管。
    let app = api.router().merge(core.router());

    // 内置 TLS：绑 https_port / http_port，装载缓存证书并起续期任务。
    if tls_settings.enabled {
        match solidway_api::tls::start(api.state(), tls_settings.clone(), app.clone()).await {
            Ok(runtime) => {
                tracing::info!(
                    https_port = tls_settings.https_port,
                    http_port = tls_settings.http_port,
                    domains = ?runtime.domains(),
                    "built-in HTTPS enabled"
                );
                if let Some(hint) = tls_settings.port_hint() {
                    tracing::warn!("{hint}");
                }
                // 续期任务与监听分离：`tls::start` 只负责监听与状态注册。
                solidway_api::tls::spawn_renewal(Arc::clone(&runtime));
            }
            Err(err) => {
                eprintln!("内置 HTTPS 启动失败: {err}");
                return ExitCode::FAILURE;
            }
        }
    }

    // 优雅关闭：停止接受新连接并等在途请求（上限 INFLIGHT_GRACE），
    // 然后执行收尾（落盘用量 + 写运行期快照，总预算 SHUTDOWN_BUDGET）。
    let mut hard_rx = shutdown_rx.clone();
    let hard_stop = async move {
        let _ = hard_rx.wait_for(|value| *value).await;
        tokio::time::sleep(INFLIGHT_GRACE).await;
    };

    // 每个 listener 一个 serve 任务，共享同一个 Router 与关闭信号。
    let mut joins = tokio::task::JoinSet::new();
    for listener in listeners {
        let app = app.clone();
        let mut rx = shutdown_rx.clone();
        joins.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.wait_for(|value| *value).await;
                })
                .await
        });
    }
    let mut serve_error: Option<String> = None;
    tokio::select! {
        _ = hard_stop => {
            tracing::warn!(seconds = INFLIGHT_GRACE.as_secs(), "in-flight requests timed out; forcing listener shutdown");
        }
        _ = async {
            while let Some(result) = joins.join_next().await {
                if let Ok(Err(err)) = result {
                    serve_error = Some(err.to_string());
                }
            }
        } => {}
    }
    if let Some(err) = serve_error {
        eprintln!("服务异常退出: {err}");
        return ExitCode::FAILURE;
    }

    match tokio::time::timeout(SHUTDOWN_BUDGET, api.graceful_shutdown()).await {
        Ok(()) => tracing::info!("shutdown complete; exiting"),
        Err(_) => tracing::warn!("shutdown timed out; forcing exit"),
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod desktop_url_tests {
    use super::local_base;

    #[test]
    fn desktop_login_url_stays_local() {
        // TLS 模式下内部监听已是 loopback。
        assert_eq!(local_base("127.0.0.1:6565"), "http://127.0.0.1:6565");
        // 通配绑定 → 回环（不能把令牌交给 0.0.0.0）。
        assert_eq!(local_base("0.0.0.0:6565"), "http://127.0.0.1:6565");
        assert_eq!(local_base("[::]:6565"), "http://[::1]:6565");
        // 只绑了具体地址时沿用该地址（没有回环可用）。
        assert_eq!(local_base("192.168.1.5:6565"), "http://192.168.1.5:6565");
    }
}
