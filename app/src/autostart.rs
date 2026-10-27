//! 开机自启动后端：把「是否登录时启动」落到当前系统的登录项。
//!
//! 抽成 trait 是为了让 `desktop::Settings` 的执行逻辑可以在不碰真实登录项的
//! 前提下做单元测试（见 `FakeBackend`）。

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as _;

/// 登录项后端。
pub trait Backend: Send + Sync {
    /// 当前系统登录项是否指向本应用。
    fn is_enabled(&self) -> Result<bool, String>;
    /// 注册登录项（已存在时为幂等覆盖）。
    fn enable(&self) -> Result<(), String>;
    /// 移除登录项。
    fn disable(&self) -> Result<(), String>;

    /// 幂等设置：已是目标状态时不动作。
    fn set(&self, enabled: bool) -> Result<(), String> {
        if self.is_enabled()? == enabled {
            return Ok(());
        }
        if enabled {
            self.enable()
        } else {
            self.disable()
        }
    }
}

/// 真实后端：`tauri-plugin-autostart`。
///
/// 插件在 setup 时按 `current_exe()` 解析注册目标，并把 `--hidden` 写进启动参数
/// （见 `main.rs` 的插件初始化），因此这里不需要关心路径。
pub struct TauriBackend {
    app: AppHandle,
}

impl TauriBackend {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl Backend for TauriBackend {
    fn is_enabled(&self) -> Result<bool, String> {
        self.app
            .autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())
    }

    fn enable(&self) -> Result<(), String> {
        self.app
            .autolaunch()
            .enable()
            .map_err(|error| error.to_string())
    }

    fn disable(&self) -> Result<(), String> {
        self.app
            .autolaunch()
            .disable()
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    use super::Backend;

    /// 可编程失败的后端，供单元测试使用。
    pub struct FakeBackend {
        enabled: Mutex<bool>,
        /// 为真时所有写操作返回错误。
        pub fail_writes: Mutex<bool>,
        /// 为真时读操作返回错误。
        pub fail_reads: Mutex<bool>,
    }

    impl FakeBackend {
        pub fn new(enabled: bool) -> Self {
            Self {
                enabled: Mutex::new(enabled),
                fail_writes: Mutex::new(false),
                fail_reads: Mutex::new(false),
            }
        }

        pub fn enabled(&self) -> bool {
            *self.enabled.lock().expect("fake lock")
        }

        pub fn set_fail_writes(&self, fail: bool) {
            *self.fail_writes.lock().expect("fake lock") = fail;
        }

        fn lock_enabled(&self) -> std::sync::MutexGuard<'_, bool> {
            self.enabled
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        }

        fn should_fail_writes(&self) -> bool {
            *self
                .fail_writes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        }

        fn should_fail_reads(&self) -> bool {
            *self
                .fail_reads
                .lock()
                .unwrap_or_else(|error| error.into_inner())
        }
    }

    impl Backend for FakeBackend {
        fn is_enabled(&self) -> Result<bool, String> {
            if self.should_fail_reads() {
                return Err("读取登录项失败".to_string());
            }
            Ok(*self.lock_enabled())
        }

        fn enable(&self) -> Result<(), String> {
            if self.should_fail_writes() {
                return Err("写入登录项被拒绝".to_string());
            }
            *self.lock_enabled() = true;
            Ok(())
        }

        fn disable(&self) -> Result<(), String> {
            if self.should_fail_writes() {
                return Err("移除登录项被拒绝".to_string());
            }
            *self.lock_enabled() = false;
            Ok(())
        }
    }
}
