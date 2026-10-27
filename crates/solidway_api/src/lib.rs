pub mod alerts;
pub mod apikey;
pub mod auth;
pub mod core_bridge;
pub mod crypto;
pub mod db;
pub mod desktop;
pub mod error;
pub mod events;
pub mod handlers;
pub mod net_holder;
pub mod provider;
pub mod routes;
pub mod selection;
pub mod settings;
pub mod state;
pub mod tls;
pub mod usage;
pub mod web_assets;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::Router;
use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

pub use error::{ApiError, Result};
pub use state::{AppState, ListenInfo, ListenSource, RunMode};

use auth::{password, session};
use db::sessions;
use db::users::{self, User};
use state::{new_id, now_unix};

/// 引导目标账号的用户名。
pub const ADMIN_USERNAME: &str = "admin";

/// 绝对化路径并解析 `.` / `..`（存在时同时解析符号链接）。
/// 用于系统信息展示与磁盘资源访问，避免出现相对路径或未展开的 `..`。
pub(crate) fn absolute_path(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        Ok(path) => {
            #[cfg(windows)]
            if let Some(stripped) = path.to_string_lossy().strip_prefix(r"\\?\") {
                return PathBuf::from(stripped);
            }
            path
        }
        // 路径尚不存在时尽力而为（不改动调用方的错误处理）。
        Err(_) => std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()),
    }
}

/// 管理面 API 的初始化参数。
pub struct ApiConfig {
    pub data_root: PathBuf,
    /// `--master-key`（hex / base64）；`None` 时使用 `<data_root>/master.key`。
    pub master_key: Option<String>,
    /// 运行模式（`--mode`）。
    pub mode: RunMode,
    /// `--web-assets` 覆盖目录。
    pub web_assets: Option<PathBuf>,
}

/// 已初始化的管理面 API。
pub struct Api {
    state: AppState,
    web_assets: web_assets::WebAssets,
    /// 本次启动签发的一次性引导令牌（仅内存，由调用方输出 stdout 事件）。
    bootstrap: Option<String>,
    /// 本次启动的桌面自动登录令牌（仅 desktop 模式；仅内存，由调用方输出 stdout 事件）。
    desktop_login: Option<String>,
}

impl Api {
    /// 建目录 → 连 SQLite → 迁移 → 载入设置快照 → 加载主密钥 → 解析静态资源 → 引导 admin。
    ///
    /// 不再读取任何配置文件：可调参数全部来自数据库（缺失用内置默认）。
    pub async fn init(config: ApiConfig) -> Result<Api> {
        let (data_root, pool) = open_pool(&config.data_root).await?;

        let masters = crypto::load_master_keys(&data_root, config.master_key.as_deref())?;
        let cipher = crypto::Cipher::new(masters)?;

        let snapshot = Arc::new(settings::load_all(&pool).await?);
        let assets =
            web_assets::WebAssets::resolve(config.web_assets).map_err(ApiError::internal)?;
        let info = assets.describe();
        let info = Arc::new(info);
        tracing::info!(
            source = info.source,
            path = info.path.as_deref().unwrap_or("-"),
            files = info.files,
            "frontend assets resolved"
        );

        let detail = crate::db::detail::DetailStore::open(&data_root).await;
        let state = AppState {
            pool,
            cipher: Arc::new(cipher),
            data_root: data_root.clone(),
            mode: config.mode,
            web_assets: Arc::clone(&info),
            settings: Arc::new(RwLock::new(snapshot)),
            listen: Arc::new(RwLock::new(None)),
            core: Arc::new(RwLock::new(None)),
            usage: Arc::new(RwLock::new(None)),
            bootstrap: Arc::new(RwLock::new(None)),
            desktop_login_hash: Arc::new(RwLock::new(None)),
            detail: Arc::new(detail),
            tls: Arc::new(RwLock::new(None)),
            shutdown: Arc::new(RwLock::new(None)),
        };
        let bootstrap = ensure_admin(&state, config.mode).await?;
        // 桌面自动登录：每次进程启动换一枚新令牌，进程内可复用；standalone 不生成。
        let desktop_login = if config.mode.is_desktop() {
            let token = session::generate_token();
            *state
                .desktop_login_hash
                .write()
                .expect("desktop login lock") = Some(session::hash_token(&token));
            Some(token)
        } else {
            None
        };
        // 首次启动固化统计时区，之后只认 DB（不随系统时区或环境变量漂移）。
        let admin_id: String =
            sqlx::query_scalar("SELECT id FROM users WHERE role = 'admin' LIMIT 1")
                .fetch_one(&state.pool)
                .await?;
        settings::ensure_usage(&state.pool, &admin_id, now_unix()).await?;
        // 历史路由没有 owner：统一归属最早的 admin（幂等）。
        let assigned = crate::db::routes::backfill_owners(&state.pool).await?;
        if assigned > 0 {
            tracing::info!(assigned, "legacy routes assigned to the earliest admin");
        }
        Ok(Api {
            state,
            web_assets: assets,
            bootstrap,
            desktop_login,
        })
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// 本次启动签发的一次性引导令牌（无则 `None`）。
    pub fn bootstrap_token(&self) -> Option<&str> {
        self.bootstrap.as_deref()
    }

    /// 本次启动的桌面自动登录令牌（standalone 模式为 `None`）。
    ///
    /// 调用方 MUST 只经 stdout 事件交付给桌面壳，MUST NOT 落日志。
    pub fn desktop_login_token(&self) -> Option<&str> {
        self.desktop_login.as_deref()
    }

    pub fn web_assets(&self) -> &web_assets::WebAssets {
        &self.web_assets
    }

    /// bind 成功后由二进制写入实际监听信息。
    pub fn set_listen(&self, info: ListenInfo) {
        self.state.set_listen(info);
    }

    /// 装配数据面引擎：读 DB → `CoreConfig` → `Core`，并启动事件消费。
    ///
    /// 返回的 `Arc<Core>` 由调用方（`solidway` 二进制）用于合并转发路由。
    pub async fn build_core(&self) -> Result<Arc<solidway_core::Core>> {
        let config = core_bridge::build_startup_config(&self.state).await?;
        let core = solidway_core::Core::new(config)
            .await
            .map_err(|error| ApiError::internal(format!("core 构建失败: {error}")))?;
        *self.state.core.write().expect("core lock") = Some(Arc::clone(&core));

        let collector = Arc::new(
            crate::usage::UsageCollector::new(self.state.pool.clone(), self.state.detail()).await,
        );
        collector.spawn_tasks();
        *self.state.usage.write().expect("usage lock") = Some(Arc::clone(&collector));

        core_bridge::spawn_event_pump(self.state.clone(), Arc::clone(&core), collector);
        Ok(core)
    }

    /// 进程收尾（落盘用量 + 写运行期快照）；由二进制的关闭流程调用。
    pub async fn graceful_shutdown(&self) {
        self.state.graceful_shutdown().await;
    }

    /// 组装完整 Router：`/api` + 静态资源（含 SPA 回退）。
    pub fn router(&self) -> Router {
        let api = routes::api_router(self.state.clone()).with_state(self.state.clone());
        Router::new()
            .nest("/api", api)
            .fallback_service(web_assets::router(self.web_assets.clone()))
    }
}

/// 建目录 → 打开 SQLite → 跑迁移（`init` 与 `--reset-password` 共用）。
async fn open_pool(data_root: &Path) -> Result<(PathBuf, SqlitePool)> {
    std::fs::create_dir_all(data_root)?;
    let data_root = absolute_path(data_root);
    // WAL + NORMAL：逐条明细是"每请求一次写"，默认（delete 日志 + FULL）会让写入受 fsync 限制。
    // WAL 下读不阻塞写、写不阻塞读；NORMAL 只保证"崩溃不损坏"，最多丢最后一个事务。
    let options = SqliteConnectOptions::new()
        .filename(data_root.join("solidway.db"))
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok((data_root, pool))
}

/// 引导页地址：`<base>/bootstrap#<token>`（令牌放 fragment，不进服务端日志与 `Referer`）。
pub fn bootstrap_url(base: &str, token: &str) -> String {
    format!("{}/bootstrap#{}", base.trim_end_matches('/'), token)
}

/// admin 引导：存在性、锁定态与令牌签发。
///
/// 返回 `Some(token)` 表示本次启动签发了一次性引导令牌，调用方须把它交给
/// stdout 事件（`events::bootstrap`），**不得**落日志。
///
/// 桌面模式不引导设口令（ADR 0017）：admin 允许以「未设置口令」状态存在，
/// 首启直接经进程令牌进入控制台，口令改为控制台内可选设置。因此该模式下
/// 只保证账号存在，MUST NOT 签发引导令牌。
async fn ensure_admin(state: &AppState, mode: RunMode) -> Result<Option<String>> {
    let now = now_unix();
    let existing = users::find_by_username(&state.pool, ADMIN_USERNAME).await?;
    let user_id = match existing {
        // 口令已可用：不创建、不签发。
        Some(user) if !password::is_locked(&user.password_hash) => return Ok(None),
        // 引导未完成的再次启动：作废旧令牌后重新签发（自愈）。
        Some(user) => user.id,
        None => {
            // 已有其他 admin 用户时不自作主张新建账号（保持原有语义）。
            if users::count_admins(&state.pool).await? > 0 {
                return Ok(None);
            }
            let user = User {
                id: new_id(),
                username: ADMIN_USERNAME.to_string(),
                // 锁定态：引导完成前任何口令都无法登录。
                password_hash: password::LOCKED_HASH.to_string(),
                role: "admin".to_string(),
                disabled: 0,
                display_name: None,
                title: None,
                created_at: now,
                updated_at: now,
                archived_at: None,
                archived_by: None,
            };
            users::insert(&state.pool, &user).await?;
            user.id
        }
    };
    if mode.is_desktop() {
        return Ok(None);
    }
    Ok(Some(issue_bootstrap(state, &user_id)))
}

/// 签发引导令牌：明文只返回给调用方，内存里也只留 SHA-256（覆盖旧令牌）。
///
/// 令牌不落盘、不跨进程传递，因此重启后旧令牌自然失效，由启动时重签兜底。
fn issue_bootstrap(state: &AppState, user_id: &str) -> String {
    let token = session::generate_token();
    let record = state::Bootstrap {
        token_hash: session::hash_token(&token),
        user_id: user_id.to_string(),
    };
    *state.bootstrap.write().expect("bootstrap lock") = Some(record);
    token
}

/// `--reset-password`：把 admin 置回「未设置口令」并作废其会话。
///
/// 不加载主密钥（不会因 `master.key` 缺失而意外生成新密钥），也不启动服务。
/// 引导令牌只活在各进程内存里，所以这里无需清理它；新的令牌由**下一次启动的
/// 进程**签发，因此本命令不产生也不消费任何明文凭证。
pub async fn reset_admin_password(data_root: &Path) -> Result<()> {
    let (_, pool) = open_pool(data_root).await?;
    let now = now_unix();
    let user = match users::find_by_username(&pool, ADMIN_USERNAME).await? {
        Some(user) => {
            users::update_password(&pool, &user.id, password::LOCKED_HASH, now).await?;
            user
        }
        None => {
            let user = User {
                id: new_id(),
                username: ADMIN_USERNAME.to_string(),
                password_hash: password::LOCKED_HASH.to_string(),
                role: "admin".to_string(),
                disabled: 0,
                display_name: None,
                title: None,
                created_at: now,
                updated_at: now,
                archived_at: None,
                archived_by: None,
            };
            users::insert(&pool, &user).await?;
            user
        }
    };
    sessions::delete_for_user(&pool, &user.id).await?;
    tracing::warn!(
        username = ADMIN_USERNAME,
        "admin password reset to the unset state"
    );
    Ok(())
}
