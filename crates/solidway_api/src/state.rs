use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use rand::Rng;
use solidway_core::Core;
use sqlx::SqlitePool;

use crate::crypto::Cipher;
use crate::settings::AllSettings;
use crate::usage::UsageCollector;

/// 运行模式：决定是否允许"由托管方重启"，以及是否启用 stdin 存活看门狗。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Desktop,
    Standalone,
}

impl RunMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::Desktop => "desktop",
            RunMode::Standalone => "standalone",
        }
    }

    pub fn is_desktop(self) -> bool {
        matches!(self, RunMode::Desktop)
    }
}

/// 实际监听信息的来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenSource {
    Database,
    CommandLine,
}

impl ListenSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ListenSource::Database => "database",
            ListenSource::CommandLine => "command_line",
        }
    }
}

/// 本次进程实际监听的地址与来源（bind 成功后由二进制写入）。
#[derive(Debug, Clone)]
pub struct ListenInfo {
    /// 实际监听的全部地址（多地址监听时可能不止一个）。
    pub effective: Vec<String>,
    pub source: ListenSource,
}

/// 一次性引导令牌：**只存在于进程内存**。
///
/// 签发方与验证方是同一进程（`ensure_admin` / `POST /api/auth/bootstrap`），
/// 令牌也不跨重启使用（明文从不落盘，锁定的 admin 每次启动重签），
/// 因此无需持久化；需跨重启保留的只有 `users.password_hash` 的锁定态。
#[derive(Debug, Clone)]
pub struct Bootstrap {
    /// 令牌的 SHA-256（内存里也不留明文，便于统一比较与日志安全）。
    pub token_hash: String,
    /// 待引导的目标用户。
    pub user_id: String,
}

/// 所有 handler 共享的应用状态。
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cipher: Arc<Cipher>,
    pub data_root: PathBuf,
    pub mode: RunMode,
    /// 静态资源来源描述（供 `/api/system` 自诊断）。
    pub web_assets: Arc<crate::web_assets::Descriptor>,
    /// 数据库快照：启动载入，写入后刷新，请求路径上不再查库。
    pub settings: Arc<RwLock<Arc<AllSettings>>>,
    /// bind 成功后写入。
    pub listen: Arc<RwLock<Option<ListenInfo>>>,
    /// 数据面引擎句柄（由 `Api::build_core` 装配后写入）。
    pub core: Arc<RwLock<Option<Arc<Core>>>>,
    /// 用量采集器（由 `Api::build_core` 创建后写入）。
    pub usage: Arc<RwLock<Option<Arc<UsageCollector>>>>,
    /// 本次进程签发的一次性引导令牌（无则 `None`；完成后置空）。
    pub bootstrap: Arc<RwLock<Option<Bootstrap>>>,
    /// 桌面自动登录令牌的 SHA-256（仅 desktop 模式；进程生命周期内可复用）。
    pub desktop_login_hash: Arc<RwLock<Option<String>>>,
    /// 明细库句柄（独立文件；打开失败时为 `None`，明细功能降级）。
    pub detail: Arc<Option<crate::db::detail::DetailStore>>,
    /// 内置 TLS 运行句柄（`tls.enabled=true` 时由二进制在绑定成功后写入）。
    pub tls: Arc<RwLock<Option<Arc<crate::tls::TlsRuntime>>>>,
    /// 统一关闭通道（由二进制启动时写入）；`/api/app/restart` 与信号共用。
    pub shutdown: Arc<RwLock<Option<tokio::sync::watch::Sender<bool>>>>,
}

impl AppState {
    /// 当前设置快照。
    pub fn settings(&self) -> Arc<AllSettings> {
        Arc::clone(&self.settings.read().expect("settings lock"))
    }

    /// 重新从数据库载入并替换快照。
    pub async fn reload_settings(&self) -> crate::Result<()> {
        let fresh = Arc::new(crate::settings::load_all(&self.pool).await?);
        *self.settings.write().expect("settings lock") = fresh;
        Ok(())
    }

    /// 刷新用量采集器的路由归属缓存（路由创建 / 转移 / offboard 后调用）。
    pub async fn refresh_usage_owners(&self) {
        let collector = {
            let guard = self.usage.read().expect("usage lock");
            guard.clone()
        };
        if let Some(collector) = collector {
            collector.refresh_owners().await;
        }
    }

    /// 明细库句柄（不可用时返回 `None`）。
    pub fn detail(&self) -> Option<crate::db::detail::DetailStore> {
        self.detail.as_ref().clone()
    }

    /// 当前的内置 TLS 句柄（未启用时 `None`）。
    pub fn tls(&self) -> Option<Arc<crate::tls::TlsRuntime>> {
        self.tls.read().expect("tls lock").clone()
    }

    /// 记录实际监听信息。
    pub fn set_listen(&self, info: ListenInfo) {
        *self.listen.write().expect("listen lock") = Some(info);
    }

    /// 安装统一关闭通道（二进制启动时调用；测试可不安装）。
    pub fn install_shutdown(&self, tx: tokio::sync::watch::Sender<bool>) {
        *self.shutdown.write().expect("shutdown lock") = Some(tx);
    }

    /// 触发统一关闭流程；返回是否已安装关闭通道。
    pub fn request_shutdown(&self) -> bool {
        self.shutdown
            .read()
            .expect("shutdown lock")
            .as_ref()
            .is_some_and(|tx| tx.send(true).is_ok())
    }
}

/// 运行期快照保留的粘性条目上限（按过期时刻倒序截断）。
pub const AFFINITY_SNAPSHOT_LIMIT: usize = 10_000;

impl AppState {
    /// 进程收尾：落盘用量 + 写运行期状态快照。
    ///
    /// 幂等、不抛错（失败只告警），调用方用超时包住即可。
    pub async fn graceful_shutdown(&self) {
        let collector = {
            let guard = self.usage.read().expect("usage lock");
            guard.clone()
        };
        if let Some(collector) = &collector {
            collector.flush_now().await;
            tracing::info!("usage buffers flushed");
        }

        let core = {
            let guard = self.core.read().expect("core lock");
            guard.clone()
        };
        if let Some(core) = core {
            let seeds = core.export_affinity(AFFINITY_SNAPSHOT_LIMIT).await;
            let payload = crate::db::runtime_snapshot::SnapshotPayload { affinity: seeds };
            match crate::db::runtime_snapshot::save(&self.pool, &payload, now_unix()).await {
                Ok(()) => {
                    tracing::info!(entries = payload.affinity.len(), "runtime snapshot written")
                }
                Err(error) => tracing::error!(%error, "failed to write runtime snapshot"),
            }
        }
        // R3-F4：快照写入期间事件上收仍可能追加 buffer，收尾再刷一次。
        if let Some(collector) = &collector {
            collector.flush_now().await;
            tracing::info!("usage buffers flushed after runtime snapshot");
        }
    }
}

/// 当前 Unix 秒。
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 生成新的记录标识。
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 路由 ID：9 位数字（首位非 0）。只用于新路由；存量 UUID 保持不变。
pub fn new_route_id() -> String {
    let mut rng = rand::thread_rng();
    let mut id = String::with_capacity(9);
    id.push(char::from_digit(rng.gen_range(1..=9), 10).expect("digit"));
    for _ in 0..8 {
        id.push(char::from_digit(rng.gen_range(0..=9), 10).expect("digit"));
    }
    id
}
