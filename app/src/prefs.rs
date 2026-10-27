//! 桌面壳偏好：`<data_root>/desktop.toml`。
//!
//! 壳是唯一写者：启动时把生效值**物化**写入，之后只在用户改动时重写。
//! `solidway` 服务只读该文件（用于控制台展示），因此
//! 「文件内容 == 壳实际执行的行为」是硬约束：应用失败时不得写入新值。

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 偏好文件名（位于 `data_root`）。
pub const FILE: &str = "desktop.toml";
/// 文件结构版本（仅用于将来迁移；当前只写不读）。
const SCHEMA: u32 = 1;

/// 桌面壳偏好。`None` 表示文件里没有该字段，取值时回落到默认值。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefs {
    pub autostart: Option<bool>,
    pub close_window_to_tray: Option<bool>,
}

/// `desktop.toml` 的实际形状：字段全可缺省，未知字段忽略（`schema` 等不进结构体）。
#[derive(Debug, Deserialize)]
struct FileShape {
    #[serde(default)]
    autostart: Option<bool>,
    #[serde(default)]
    close_window_to_tray: Option<bool>,
}

/// 加载结果：`state != Ok` 时启动流程要物化写回。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    /// 文件存在且两个字段都在。
    Ok,
    /// 文件不存在。
    Missing,
    /// 文件存在但缺少字段。
    Partial,
    /// 文件存在但无法解析。
    Unreadable,
}

/// 一次加载的产物。
pub struct Loaded {
    pub prefs: Prefs,
    pub state: LoadState,
}

impl Prefs {
    /// 读取偏好。任何失败都回落到 `None`（即默认值），不阻止启动。
    pub fn load(data_root: &Path) -> Loaded {
        let Ok(text) = std::fs::read_to_string(path(data_root)) else {
            return Loaded {
                prefs: Prefs::default(),
                state: LoadState::Missing,
            };
        };
        let Ok(shape) = toml::from_str::<FileShape>(&text) else {
            return Loaded {
                prefs: Prefs::default(),
                state: LoadState::Unreadable,
            };
        };
        let state = if shape.autostart.is_none() || shape.close_window_to_tray.is_none() {
            LoadState::Partial
        } else {
            LoadState::Ok
        };
        Loaded {
            prefs: Prefs {
                autostart: shape.autostart,
                close_window_to_tray: shape.close_window_to_tray,
            },
            state,
        }
    }

    /// 生效值：开机自启动（缺失时按构建类型的默认值）。
    pub fn autostart(&self) -> bool {
        self.autostart.unwrap_or_else(default_autostart)
    }

    /// 生效值：关闭窗口时最小化到托盘（默认开启）。
    pub fn close_to_tray(&self) -> bool {
        self.close_window_to_tray.unwrap_or(true)
    }

    pub fn set_autostart(&mut self, on: bool) {
        self.autostart = Some(on);
    }

    pub fn set_close_to_tray(&mut self, on: bool) {
        self.close_window_to_tray = Some(on);
    }

    /// 物化 / 保存：把生效值原子写入 `desktop.toml`。
    pub fn save(&self, data_root: &Path) -> Result<(), String> {
        let target = path(data_root);
        let temp = data_root.join(format!("{FILE}.tmp"));
        let text = format!(
            "# 由 Solidway 桌面壳维护；手工修改后需重启应用生效。\n\
             schema = {SCHEMA}\n\
             autostart = {}\n\
             close_window_to_tray = {}\n",
            self.autostart(),
            self.close_to_tray()
        );
        std::fs::write(&temp, text)
            .map_err(|error| format!("写入 {} 失败：{error}", temp.display()))?;
        if let Err(error) = std::fs::rename(&temp, &target) {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("提交 {} 失败：{error}", target.display()));
        }
        Ok(())
    }
}

/// 默认开机自启动：发布构建开启，debug 构建关闭（避免把开发者登录项指向构建产物）。
pub fn default_autostart() -> bool {
    !cfg!(debug_assertions)
}

fn path(data_root: &Path) -> PathBuf {
    data_root.join(FILE)
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
            "solidway-app-prefs-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        root
    }

    #[test]
    fn missing_file_uses_defaults() {
        let root = temp_root("missing");
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Missing);
        assert_eq!(loaded.prefs.autostart(), default_autostart());
        assert!(loaded.prefs.close_to_tray());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn broken_file_falls_back_to_defaults() {
        let root = temp_root("broken");
        std::fs::write(root.join(FILE), "autostart = [oops\n").expect("write");
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Unreadable);
        assert_eq!(loaded.prefs.autostart(), default_autostart());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn partial_file_is_reported_for_materialization() {
        let root = temp_root("partial");
        std::fs::write(root.join(FILE), "autostart = false\n").expect("write");
        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Partial);
        assert!(!loaded.prefs.autostart());
        assert!(loaded.prefs.close_to_tray());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn save_roundtrips_and_leaves_no_temp_file() {
        let root = temp_root("save");
        let mut prefs = Prefs::default();
        prefs.set_autostart(false);
        prefs.set_close_to_tray(true);
        prefs.save(&root).expect("save");

        let loaded = Prefs::load(&root);
        assert_eq!(loaded.state, LoadState::Ok);
        assert_eq!(loaded.prefs, prefs);
        assert!(!root.join(format!("{FILE}.tmp")).exists());
        std::fs::remove_dir_all(&root).ok();
    }
}
