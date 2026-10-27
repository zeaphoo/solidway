//! 桌面偏好的执行层：系统登录项、关窗策略与回执文件。
//!
//! 不变式（与 `crates/solidway_api/src/desktop.rs` 的只读视图配套）：
//!
//! - `desktop.toml` 恒等于壳**实际执行**的行为；
//! - `desktop.status.json` 恒为最近一次 `apply` 的结果。
//!
//! 执行失败时**不写偏好文件**（保持旧值），只写失败回执：控制台据此回滚开关。

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::menu::CheckMenuItem;
use tauri::Wry;

use crate::autostart::Backend;
use crate::prefs::Prefs;

/// 回执文件名（位于 `data_root`）。
pub const STATUS_FILE: &str = "desktop.status.json";
/// 壳级日志文件名（与子进程的 `solidway.log` 分开）。
pub const LOG_FILE: &str = "desktop.log";

/// 托盘勾选项句柄：偏好变化（控制台或托盘）后同步勾选状态。
#[derive(Default)]
struct Items {
    autostart: Option<CheckMenuItem<Wry>>,
    close_to_tray: Option<CheckMenuItem<Wry>>,
}

/// 桌面设置：偏好 + 后端 + 回执写入。
pub struct Settings {
    data_root: PathBuf,
    backend: Box<dyn Backend>,
    prefs: Mutex<Prefs>,
    items: Mutex<Items>,
}

impl Settings {
    pub fn new(data_root: PathBuf, backend: Box<dyn Backend>, prefs: Prefs) -> Self {
        Self {
            data_root,
            backend,
            prefs: Mutex::new(prefs),
            items: Mutex::new(Items::default()),
        }
    }

    /// 绑定托盘勾选项（在 `apply` 时同步勾选状态）。
    pub fn attach_items(&self, autostart: CheckMenuItem<Wry>, close_to_tray: CheckMenuItem<Wry>) {
        let mut items = self.items.lock().unwrap_or_else(|error| error.into_inner());
        items.autostart = Some(autostart);
        items.close_to_tray = Some(close_to_tray);
    }

    pub fn autostart(&self) -> bool {
        self.lock_prefs().autostart()
    }

    pub fn close_to_tray(&self) -> bool {
        self.lock_prefs().close_to_tray()
    }

    /// 启动对账：按偏好把系统登录项拉回一致（幂等）。
    pub fn reconcile(&self) {
        let (autostart, close_to_tray) = self.effective();
        // 失败已在 apply 内写回执与日志，这里不再重复提示。
        let _ = self.apply(autostart, close_to_tray, None);
    }

    /// 托盘开关：只改开机自启动，保留当前关窗策略。
    pub fn set_autostart(&self, on: bool) -> Result<(), String> {
        let close_to_tray = self.close_to_tray();
        self.apply(on, close_to_tray, None)
    }

    /// 托盘开关：只改关窗策略，保留当前自启动状态。
    pub fn set_close_to_tray(&self, on: bool) -> Result<(), String> {
        let autostart = self.autostart();
        self.apply(autostart, on, None)
    }

    /// 应用一组偏好。`apply_id` 来自控制台的 `desktop-apply` 事件；托盘 / 对账为 `None`。
    ///
    /// 顺序：先动系统登录项（可失败）→ 再落盘 → 再写回执。任一步失败都**不改**偏好文件，
    /// 回执 `ok = false` 并带可读原因。
    pub fn apply(
        &self,
        autostart: bool,
        close_to_tray: bool,
        apply_id: Option<&str>,
    ) -> Result<(), String> {
        let current = self.lock_prefs().clone();

        // ① 系统登录项：失败即整体回滚（关窗策略也不改），保证 UI 只有「全成功 / 全失败」。
        if let Err(error) = self.backend.set(autostart) {
            let mut applied = current.clone();
            if let Ok(actual) = self.backend.is_enabled() {
                applied.set_autostart(actual);
            }
            let message = format!("无法调整开机自启动：{error}");
            self.write_status(apply_id, false, Some(&message), &applied);
            log(&self.data_root, &message);
            return Err(message);
        }

        // ② 以系统读回为准（抵消并发 / 竞态），再落盘。
        let actual_autostart = self.backend.is_enabled().unwrap_or(autostart);
        let mut next = current.clone();
        next.set_autostart(actual_autostart);
        next.set_close_to_tray(close_to_tray);

        if let Err(error) = next.save(&self.data_root) {
            let message = format!("无法写入桌面偏好：{error}");
            // 文件仍是旧值，但系统登录项可能已变：回执以现实为准，下次启动对账自愈。
            let mut applied = current.clone();
            applied.set_autostart(actual_autostart);
            self.write_status(apply_id, false, Some(&message), &applied);
            log(&self.data_root, &message);
            return Err(message);
        }

        *self.lock_prefs() = next.clone();
        self.sync_items();
        self.write_status(apply_id, true, None, &next);
        Ok(())
    }

    fn effective(&self) -> (bool, bool) {
        let prefs = self.lock_prefs();
        (prefs.autostart(), prefs.close_to_tray())
    }

    fn lock_prefs(&self) -> std::sync::MutexGuard<'_, Prefs> {
        self.prefs.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn sync_items(&self) {
        let (autostart, close_to_tray) = self.effective();
        let items = self.items.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(item) = &items.autostart {
            let _ = item.set_checked(autostart);
        }
        if let Some(item) = &items.close_to_tray {
            let _ = item.set_checked(close_to_tray);
        }
    }

    fn write_status(&self, apply_id: Option<&str>, ok: bool, error: Option<&str>, applied: &Prefs) {
        let body = serde_json::json!({
            "apply_id": apply_id,
            "ok": ok,
            "error": error,
            "applied": {
                "autostart": applied.autostart(),
                "close_window_to_tray": applied.close_to_tray(),
            },
            "at": now_unix(),
        });
        let target = self.data_root.join(STATUS_FILE);
        let temp = self.data_root.join(format!("{STATUS_FILE}.tmp"));
        let result =
            std::fs::write(&temp, body.to_string()).and_then(|()| std::fs::rename(&temp, &target));
        if let Err(error) = result {
            let _ = std::fs::remove_file(&temp);
            // 回执写不出去只能记日志：控制台会因超时提示，下次操作可重试。
            log(
                &self.data_root,
                &format!("写入 {STATUS_FILE} 失败：{error}"),
            );
        }
    }
}

/// 追加一行壳级日志（`desktop.log`），并同步到 stderr 便于终端观察。
pub fn log(data_root: &Path, message: &str) {
    use std::io::Write as _;

    let line = format!("[{}] {message}\n", now_unix());
    let opened = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(data_root.join(LOG_FILE));
    if let Ok(mut file) = opened {
        let _ = file.write_all(line.as_bytes());
    }
    eprintln!("{message}");
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::autostart::fake::FakeBackend;

    fn temp_root(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "solidway-app-desktop-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        root
    }

    fn make_settings(root: &Path, backend: Arc<FakeBackend>) -> Settings {
        struct Shared(Arc<FakeBackend>);
        impl Backend for Shared {
            fn is_enabled(&self) -> Result<bool, String> {
                self.0.is_enabled()
            }
            fn enable(&self) -> Result<(), String> {
                self.0.enable()
            }
            fn disable(&self) -> Result<(), String> {
                self.0.disable()
            }
        }
        let mut prefs = Prefs::default();
        prefs.set_autostart(true);
        prefs.set_close_to_tray(true);
        Settings::new(root.to_path_buf(), Box::new(Shared(backend)), prefs)
    }

    fn read_status(root: &Path) -> serde_json::Value {
        let text = std::fs::read_to_string(root.join(STATUS_FILE)).expect("status file");
        serde_json::from_str(&text).expect("status json")
    }

    #[test]
    fn apply_writes_prefs_and_ok_status() {
        let root = temp_root("ok");
        let backend = Arc::new(FakeBackend::new(false));
        let settings = make_settings(&root, Arc::clone(&backend));

        settings
            .apply(true, false, Some("3f2a1c9d"))
            .expect("apply should succeed");

        assert!(backend.enabled());
        let loaded = Prefs::load(&root);
        assert!(loaded.prefs.autostart());
        assert!(!loaded.prefs.close_to_tray());
        let status = read_status(&root);
        assert_eq!(status["apply_id"], "3f2a1c9d");
        assert_eq!(status["ok"], true);
        assert_eq!(status["applied"]["autostart"], true);
        assert_eq!(status["applied"]["close_window_to_tray"], false);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn failed_backend_keeps_prefs_and_reports_error() {
        let root = temp_root("fail");
        let backend = Arc::new(FakeBackend::new(true));
        let settings = make_settings(&root, Arc::clone(&backend));
        settings.apply(true, true, None).expect("seed");

        backend.set_fail_writes(true);
        let error = settings
            .apply(false, false, Some("deadbeef"))
            .expect_err("apply should fail");
        assert!(error.contains("无法调整开机自启动"), "{error}");

        // 偏好文件保持旧值（不是请求的新值）。
        let loaded = Prefs::load(&root);
        assert!(loaded.prefs.autostart());
        assert!(loaded.prefs.close_to_tray());

        let status = read_status(&root);
        assert_eq!(status["apply_id"], "deadbeef");
        assert_eq!(status["ok"], false);
        assert_eq!(status["applied"]["autostart"], true);
        assert!(status["error"]
            .as_str()
            .is_some_and(|text| !text.is_empty()));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reconcile_heals_missing_login_item() {
        let root = temp_root("reconcile");
        let backend = Arc::new(FakeBackend::new(false));
        let settings = make_settings(&root, Arc::clone(&backend));

        settings.reconcile();

        assert!(backend.enabled(), "偏好为开启时应重建登录项");
        let status = read_status(&root);
        assert!(status["apply_id"].is_null(), "对账不携带 apply_id");
        assert_eq!(status["ok"], true);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicate_apply_is_idempotent() {
        let root = temp_root("idempotent");
        let backend = Arc::new(FakeBackend::new(false));
        let settings = make_settings(&root, Arc::clone(&backend));

        settings
            .apply(true, false, Some("aaaa0001"))
            .expect("first");
        settings
            .apply(true, false, Some("aaaa0001"))
            .expect("second");

        let status = read_status(&root);
        assert_eq!(status["apply_id"], "aaaa0001");
        assert_eq!(status["ok"], true);
        std::fs::remove_dir_all(&root).ok();
    }
}
