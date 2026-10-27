//! 桌面壳偏好与回执的**只读**视图。
//!
//! 真相由桌面壳持有：壳是 `desktop.toml`（偏好）与 `desktop.status.json`（回执）的
//! 唯一写者。服务端只读这两个文件，供控制台展示当前值与等待「已生效」回执；
//! 服务端 MUST NOT 写它们——否则会与壳形成双写者，破坏
//! 「文件内容 == 壳实际执行的行为」这一不变式。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 偏好文件名（位于 `data_root`）。
pub const PREFS_FILE: &str = "desktop.toml";
/// 回执文件名（位于 `data_root`）。
pub const STATUS_FILE: &str = "desktop.status.json";

/// 桌面壳偏好。字段名与 `desktop.toml` / `desktop.status.json` 一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefs {
    pub autostart: bool,
    pub close_window_to_tray: bool,
}

impl Prefs {
    /// 缺省值：两项都开启（与发布构建一致；debug 构建的差异只在壳侧）。
    pub fn defaults() -> Self {
        Self {
            autostart: true,
            close_window_to_tray: true,
        }
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Self::defaults()
    }
}

/// 偏好值的来源，供界面区分「文件里的值」与「默认值」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefsSource {
    /// 文件存在且可解析；缺字段的项已由默认值补位。
    File,
    /// 文件不存在。
    Default,
    /// 文件存在但无法解析（不回退到部分字段，整体按默认值）。
    Unreadable,
}

/// `desktop.toml` 的宽松形状：字段全可缺省，未知字段忽略（`schema` 等不进结构体）。
#[derive(Debug, Deserialize)]
struct PrefsFile {
    #[serde(default)]
    autostart: Option<bool>,
    #[serde(default)]
    close_window_to_tray: Option<bool>,
}

/// 读取偏好。缺文件 → 默认值；坏文件 → 默认值（`Unreadable`）；缺字段 → 默认值补位。
pub fn load_prefs(data_root: &Path) -> (Prefs, PrefsSource) {
    let text = match std::fs::read_to_string(prefs_path(data_root)) {
        Ok(text) => text,
        Err(_) => return (Prefs::defaults(), PrefsSource::Default),
    };
    let Ok(file) = toml::from_str::<PrefsFile>(&text) else {
        return (Prefs::defaults(), PrefsSource::Unreadable);
    };
    let defaults = Prefs::defaults();
    (
        Prefs {
            autostart: file.autostart.unwrap_or(defaults.autostart),
            close_window_to_tray: file
                .close_window_to_tray
                .unwrap_or(defaults.close_window_to_tray),
        },
        PrefsSource::File,
    )
}

/// 壳执行后的回执。`apply_id` 为 `None` 表示托盘操作或启动对账。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    #[serde(default)]
    pub apply_id: Option<String>,
    pub ok: bool,
    #[serde(default)]
    pub error: Option<String>,
    pub applied: Prefs,
    pub at: i64,
}

/// 读取最近一次回执。文件缺失或无法解析 → `None`（界面按「尚无回执」处理）。
pub fn load_status(data_root: &Path) -> Option<Status> {
    let text = std::fs::read_to_string(status_path(data_root)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn prefs_path(data_root: &Path) -> PathBuf {
    data_root.join(PREFS_FILE)
}

pub fn status_path(data_root: &Path) -> PathBuf {
    data_root.join(STATUS_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "solidway-desktop-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        root
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let root = temp_root("missing");
        let (prefs, source) = load_prefs(&root);
        assert_eq!(prefs, Prefs::defaults());
        assert_eq!(source, PrefsSource::Default);
        assert!(load_status(&root).is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn partial_file_uses_defaults_for_missing_fields() {
        let root = temp_root("partial");
        std::fs::write(prefs_path(&root), "autostart = false\n").expect("write");
        let (prefs, source) = load_prefs(&root);
        assert_eq!(source, PrefsSource::File);
        assert!(!prefs.autostart);
        assert!(prefs.close_window_to_tray);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let root = temp_root("unknown");
        std::fs::write(
            prefs_path(&root),
            "schema = 1\nautostart = true\nclose_window_to_tray = false\nfuture = \"x\"\n",
        )
        .expect("write");
        let (prefs, source) = load_prefs(&root);
        assert_eq!(source, PrefsSource::File);
        assert!(prefs.autostart);
        assert!(!prefs.close_window_to_tray);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn broken_file_is_unreadable_and_uses_defaults() {
        let root = temp_root("broken");
        std::fs::write(prefs_path(&root), "autostart = [not a bool\n").expect("write");
        let (prefs, source) = load_prefs(&root);
        assert_eq!(source, PrefsSource::Unreadable);
        assert_eq!(prefs, Prefs::defaults());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn status_roundtrip_and_broken_status() {
        let root = temp_root("status");
        assert!(load_status(&root).is_none());

        let status = Status {
            apply_id: Some("3f2a1c9d".to_string()),
            ok: false,
            error: Some("系统拒绝写入登录项".to_string()),
            applied: Prefs {
                autostart: false,
                close_window_to_tray: true,
            },
            at: 1_761_700_000,
        };
        std::fs::write(
            status_path(&root),
            serde_json::to_string(&status).expect("serialize"),
        )
        .expect("write");
        let loaded = load_status(&root).expect("status");
        assert_eq!(loaded.apply_id.as_deref(), Some("3f2a1c9d"));
        assert!(!loaded.ok);
        assert!(!loaded.applied.autostart);

        std::fs::write(status_path(&root), "{ not json").expect("write");
        assert!(load_status(&root).is_none());
        std::fs::remove_dir_all(&root).ok();
    }
}
