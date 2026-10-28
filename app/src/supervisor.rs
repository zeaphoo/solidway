//! `solidway` 子进程的启动、事件解析、健康监控、重启与退出清理。
//!
//! 端口**不来自配置文件**：子进程在 bind 成功后会输出
//! `solidway: listening addr=host:port`，壳据此得知实际地址。
//!
//! # 线程约定
//!
//! GTK 只允许在主线程创建/操作窗口，而监控与 stdout 读取跑在后台线程。
//! 因此所有涉及窗口的公开入口都会把动作投递到主线程（`dispatch`），内部以
//! `*_now` 结尾的方法只在主线程调用。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::menu::MenuItem;
use tauri::{
    AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry,
};

use crate::config::{self, AppConfig};
use crate::desktop::Settings;

const SPLASH: &str = "splash";
const MAIN: &str = "main";
/// 托盘图标 id（`main` 与状态更新共用）。
pub const TRAY_ID: &str = "solidway";
/// splash 窗口尺寸（`main` 复用）。
pub const SPLASH_SIZE: (f64, f64) = (520.0, 380.0);
/// 控制台窗口初始尺寸。
const CONSOLE_SIZE: (f64, f64) = (1280.0, 840.0);
/// 失败自动重启的退避次数上限。
const MAX_RESTARTS: u32 = 5;
/// 引导事件的 stdout 前缀：**含一次性令牌**，一律不写日志。
const BOOTSTRAP_PREFIX: &str = "solidway: bootstrap ";
/// 桌面自动登录事件前缀：**含可复用令牌**，一律不写日志。
const DESKTOP_LOGIN_PREFIX: &str = "solidway: desktop-login ";
/// 等待 `listening` 事件的时限。
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// 当前子进程 pid，供信号处理器在进程被 SIGTERM 时同步清理。
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

/// 安装信号处理器：被 SIGTERM/SIGINT/SIGHUP 中断时先杀掉子进程再退出。
#[cfg(unix)]
pub fn install_signal_handlers() {
    extern "C" fn handle(_signal: libc::c_int) {
        let pid = CHILD_PID.load(Ordering::SeqCst);
        if pid > 0 {
            // kill / _exit 均为 async-signal-safe。
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        unsafe { libc::_exit(0) }
    }

    unsafe {
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(signal, handle as *const () as libc::sighandler_t);
        }
    }
}

/// 非 Unix 平台无对应需处理的终端信号。
#[cfg(not(unix))]
pub fn install_signal_handlers() {}

/// 控制台导航优先级：引导页（未设口令）→ 桌面自动登录 → 普通控制台。
fn console_target(
    bootstrap_url: Option<&str>,
    desktop_login_url: Option<&str>,
    addr: Option<&str>,
) -> Option<String> {
    if let Some(url) = bootstrap_url {
        return Some(url.to_string());
    }
    if let Some(url) = desktop_login_url {
        return Some(url.to_string());
    }
    addr.map(|addr| format!("http://{addr}/"))
}

/// 「打开控制台」但手上没有地址时的处置。
///
/// 关键约束：**不要用通用文案覆盖已有失败原因**（例如「未找到 solidway」
/// 「端口被占用」「启动超时」），否则用户看到的永远是最没信息量的那一句。
#[derive(Debug, PartialEq, Eq)]
enum OpenAction {
    /// 有可用地址：创建 / 聚焦控制台窗口
    Open,
    /// 子进程仍在握手：展示启动页，不判失败
    ShowSplash,
    /// 已有失败 / 停止原因：展示状态页并保留原因
    ShowFailure,
    /// 既无地址也无原因：判定失败（提示里带日志路径）
    Fail,
}

fn open_action(has_target: bool, state: State, has_detail: bool) -> OpenAction {
    if has_target {
        return OpenAction::Open;
    }
    if state == State::Starting {
        return OpenAction::ShowSplash;
    }
    if has_detail {
        return OpenAction::ShowFailure;
    }
    OpenAction::Fail
}

/// 含令牌的事件行：只进内存，不写日志。
fn is_secret_event(line: &str) -> bool {
    line.starts_with(BOOTSTRAP_PREFIX) || line.starts_with(DESKTOP_LOGIN_PREFIX)
}

/// Dock 图标显隐（方案 A）：控制台窗口存在 → `regular`（图标可见），不存在 →
/// `accessory`（图标隐藏，应用与托盘继续运行）。判据与菜单可用性同源，恒隐藏
/// （`LSUIElement`）被否。非 macOS 无 Dock 概念，编译期即空实现。
#[cfg(target_os = "macos")]
fn apply_dock_presence(app: &AppHandle, present: bool) {
    let policy = if present {
        tauri::ActivationPolicy::Regular
    } else {
        tauri::ActivationPolicy::Accessory
    };
    if let Err(error) = app.set_activation_policy(policy) {
        eprintln!("切换 Dock 图标形态失败：{error}");
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_dock_presence(_app: &AppHandle, _present: bool) {}

/// 把控制台窗口**真正**带到前台并聚焦（所有「打开 / 重开控制台」入口共用）。
///
/// macOS 的 tao `Window::set_focus` 只在「可见且未最小化」时执行
/// `makeKeyAndOrderFront` + `NSApplication activateIgnoringOtherApps:YES`；
/// 最小化时整个调用被静默跳过（连应用激活都不做）。而关窗态是 `accessory`
/// （Dock 图标隐藏），未激活的应用仅靠 `orderFront` 只会把窗口排到当前前台应用
/// 之后——这正是「有时在最前、有时被挡住」的根因之一。
/// tauri 2.12.4 没有 app 级 activate API（`AppHandle::show()` 只 unhide、不聚焦），
/// 因此统一「先还原 → 再显示 → 最后聚焦」，由 `set_focus` 内的激活步骤完成置前。
#[cfg(target_os = "macos")]
fn bring_console_to_front(window: &WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

/// 非 macOS：保持改动前的 show + focus 行为（无最小化还原步骤）。
#[cfg(not(target_os = "macos"))]
fn bring_console_to_front(window: &WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

/// 解析事件里的布尔字段（契约是 `1` / `0`，同时容忍 `true` / `false`）。
fn flag(value: Option<&&str>) -> Option<bool> {
    match value.copied() {
        Some("1") | Some("true") => Some(true),
        Some("0") | Some("false") => Some(false),
        _ => None,
    }
}

/// 启动失败的分类：决定错误页的标题、建议文案与可用操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    /// 找不到随包分发的 solidway（安装不完整 / 被隔离）
    MissingBinary,
    /// 找到了但不可执行（缺权限 / 架构不符）
    NotExecutable,
    /// 系统拒绝启动（其他 spawn 错误）
    SpawnFailed,
    /// 端口被占用
    PortConflict,
    /// 端口无权限（<1024）
    PortForbidden,
    /// 子进程异常退出
    Crash,
    /// 握手超时（未收到 listening）
    Timeout,
}

impl FailureKind {
    /// 用户视角的（标题，建议）。
    fn copy(self) -> (&'static str, &'static str) {
        match self {
            Self::MissingBinary => (
                "安装可能不完整",
                "找不到随包分发的服务程序。常见原因：安装包下载不完整、解压时漏了文件、被杀毒软件隔离。建议重新下载安装包并安装。",
            ),
            Self::NotExecutable => (
                "服务程序无法运行",
                "内置的服务程序缺少执行权限，通常是解压方式导致。建议重新安装。",
            ),
            Self::SpawnFailed => ("服务启动失败", "系统拒绝启动内置服务程序，详情见下方。"),
            Self::PortConflict => (
                "端口被占用",
                "监听端口已被其他进程占用。请关闭占用该端口的程序后点「重试」；若占用者是另一个 solidway，说明上次没有正常退出。",
            ),
            Self::PortForbidden => (
                "没有权限使用该端口",
                "1000 以下的端口（如 80/443）需要特权。请改用 1024 以上的端口，或调整运行权限。",
            ),
            Self::Crash => ("服务启动失败", "服务进程异常退出，日志末尾见下方。"),
            Self::Timeout => ("服务没有按时就绪", "等待超时仍未收到监听事件，日志末尾见下方。"),
        }
    }
}

/// spawn 的 io 错误 → 分类。
fn classify_spawn_error(kind: std::io::ErrorKind) -> FailureKind {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::NotFound => FailureKind::MissingBinary,
        ErrorKind::PermissionDenied => FailureKind::NotExecutable,
        _ => FailureKind::SpawnFailed,
    }
}

/// `bind-failed` 的 kind 字段 → 分类（权限不足单独给文案）。
fn classify_bind(kind: &str) -> FailureKind {
    if kind == "permission-denied" {
        FailureKind::PortForbidden
    } else {
        FailureKind::PortConflict
    }
}

/// 日志末尾若干行（去 ANSI、按字符截断），用于错误页内嵌。
fn log_tail(data_root: &std::path::Path, lines: usize) -> String {
    let Ok(text) = std::fs::read_to_string(data_root.join("solidway.log")) else {
        return String::new();
    };
    let tail: Vec<&str> = {
        let all: Vec<&str> = text.lines().collect();
        let start = all.len().saturating_sub(lines);
        all[start..].to_vec()
    };
    let joined = strip_ansi(&tail.join("\n"));
    let count = joined.chars().count();
    if count > 2000 {
        joined.chars().skip(count - 2000).collect()
    } else {
        joined
    }
}

/// 去掉 ANSI 颜色控制序列（日志里带 tracing 的颜色码）。
fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if code.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Running,
    Stopped,
    Failed,
}

impl State {
    fn label(self, addr: Option<&str>) -> String {
        match self {
            State::Starting => "启动中…".to_string(),
            State::Running => match addr {
                Some(addr) => format!("运行中 · {addr}"),
                None => "运行中".to_string(),
            },
            State::Stopped => "已停止".to_string(),
            State::Failed => "异常".to_string(),
        }
    }
}

struct Inner {
    state: State,
    status: String,
    detail: Option<String>,
    /// 失败分类（成功启动 / 运行中为 None），决定错误页展示的按钮。
    failure: Option<FailureKind>,
    child: Option<Child>,
    generation: u64,
    restarts: u32,
    /// 本次进程签发的引导页地址（`/bootstrap#<token>`）；引导完成后不再变化。
    bootstrap_url: Option<String>,
    /// 本次进程的桌面自动登录地址（`/desktop#<token>`）；服务重启后重新签发。
    desktop_login_url: Option<String>,
    started_at: Instant,
    /// 由 `listening` 事件写入的实际监听地址。
    addr: Option<String>,
    /// 由 `bind-failed` 事件写入的可读说明。
    bind_failure: Option<String>,
}

/// solidway 子进程监督器。
pub struct Supervisor {
    config: AppConfig,
    bin: Result<PathBuf, String>,
    status_item: MenuItem<Wry>,
    inner: Mutex<Inner>,
}

impl Supervisor {
    pub fn new(
        config: AppConfig,
        bin: Result<PathBuf, String>,
        status_item: MenuItem<Wry>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            bin,
            status_item,
            inner: Mutex::new(Inner {
                state: State::Starting,
                status: "正在启动 Solidway…".to_string(),
                detail: None,
                child: None,
                generation: 0,
                restarts: 0,
                bootstrap_url: None,
                desktop_login_url: None,
                started_at: Instant::now(),
                addr: None,
                bind_failure: None,
                failure: None,
            }),
        })
    }

    // ---------------------------------------------------------------- 公开入口

    /// 启动子进程并开始监控。
    pub fn start(self: &Arc<Self>, app: &AppHandle) {
        self.dispatch(app, |supervisor, app| supervisor.start_now(app));
    }

    /// 停止当前子进程并重新启动（托盘「重启」）。
    pub fn restart(self: &Arc<Self>, app: &AppHandle) {
        self.dispatch(app, |supervisor, app| {
            supervisor.kill_child();
            supervisor.lock().restarts = 0;
            supervisor.start_now(app);
        });
    }

    /// 打开（或聚焦）控制台窗口。
    pub fn open_console(self: &Arc<Self>, app: &AppHandle) {
        self.dispatch(app, |supervisor, app| supervisor.open_console_now(app));
    }

    /// 托盘「以管理员自动登录」：把控制台导航到桌面登录地址后重新建立会话。
    pub fn admin_login(self: &Arc<Self>, app: &AppHandle) {
        self.dispatch(app, |supervisor, app| supervisor.admin_login_now(app));
    }

    /// 停止子进程（退出 / 清理）。可在任意线程调用。
    pub fn shutdown(&self) {
        self.kill_child();
    }

    /// 把当前状态注入 splash 页面（供页面加载完成后调用）。
    pub fn apply_status_to(&self, window: &WebviewWindow) {
        let (status, detail, failure) = {
            let inner = self.lock();
            (inner.status.clone(), inner.detail.clone(), inner.failure)
        };
        let text = serde_json::to_string(&status).unwrap_or_else(|_| "\"\"".to_string());
        let detail = match detail {
            Some(detail) => serde_json::to_string(&detail).unwrap_or_else(|_| "null".to_string()),
            None => "null".to_string(),
        };
        let (hint, failed) = match failure {
            Some(kind) => {
                let (_, hint) = kind.copy();
                (
                    serde_json::to_string(hint).unwrap_or_else(|_| "null".to_string()),
                    "true",
                )
            }
            None => ("null".to_string(), "false"),
        };
        let _ = window.eval(format!(
            "window.solidwaySetStatus && window.solidwaySetStatus({text}, {hint}, {detail}, {failed});"
        ));
    }

    // ------------------------------------------------------------ 主线程实现

    /// 把动作投递到主线程执行（窗口操作与子进程派生都必须在主线程）。
    fn dispatch<F>(self: &Arc<Self>, app: &AppHandle, action: F)
    where
        F: FnOnce(&Arc<Self>, &AppHandle) + Send + 'static,
    {
        let supervisor = Arc::clone(self);
        let handle = app.clone();
        if let Err(error) = app.run_on_main_thread(move || action(&supervisor, &handle)) {
            eprintln!("投递主线程任务失败：{error}");
        }
    }

    fn start_now(self: &Arc<Self>, app: &AppHandle) {
        if let Some(window) = app.get_webview_window(MAIN) {
            let _ = window.close();
        }
        // 静默启动（登录自启）不建 splash；失败路径仍会建，错误可见性优先。
        if !self.config.hidden {
            self.ensure_splash(app);
        }

        let log_path = self.config.data_root.join("solidway.log");

        let mut child = match self.spawn_child(&log_path) {
            Ok(child) => child,
            Err(error) => {
                let (kind, message) = error;
                self.set_state(State::Failed, "启动失败", Some(message));
                self.lock().failure = Some(kind);
                self.ensure_splash(app);
                self.update_ui(app);
                return;
            }
        };
        let stdout = child.stdout.take();

        let generation = {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.state = State::Starting;
            inner.status = "正在启动 Solidway…".to_string();
            inner.detail = None;
            inner.addr = None;
            inner.bind_failure = None;
            inner.failure = None;
            inner.child = Some(child);
            inner.bootstrap_url = None;
            inner.desktop_login_url = None;
            inner.started_at = Instant::now();
            inner.generation
        };

        if let Some(stdout) = stdout {
            self.spawn_reader(stdout, app.clone(), generation, log_path);
        }

        self.update_ui(app);
        self.monitor(app.clone(), generation);
    }

    fn open_console_now(self: &Arc<Self>, app: &AppHandle) {
        if let Some(window) = app.get_webview_window(MAIN) {
            // 已存在：只显示 + 聚焦，不导航、不刷新（D1）；macOS 需还原最小化并激活应用。
            bring_console_to_front(&window);
            return;
        }
        // 首次引导期间直接进设置页（地址由服务端事件给出，不在这里拼令牌）。
        let (bootstrap_url, desktop_login_url, addr, state, has_detail) = {
            let inner = self.lock();
            (
                inner.bootstrap_url.clone(),
                inner.desktop_login_url.clone(),
                inner.addr.clone(),
                inner.state,
                inner.detail.is_some(),
            )
        };
        let target = console_target(
            bootstrap_url.as_deref(),
            desktop_login_url.as_deref(),
            addr.as_deref(),
        );
        let target = match open_action(target.is_some(), state, has_detail) {
            OpenAction::Open => target.expect("Open 前置条件：存在目标地址"),
            // 启动中 / 已有原因：只把状态页带到前台，保留已有 detail。
            OpenAction::ShowSplash | OpenAction::ShowFailure => {
                self.ensure_splash(app);
                self.update_ui(app);
                return;
            }
            OpenAction::Fail => {
                let log = self.config.data_root.join("solidway.log");
                self.failed(
                    app,
                    format!(
                        "尚未获知监听地址（未收到 listening 事件）\n\n日志：{}",
                        log.display()
                    ),
                );
                return;
            }
        };
        if let Some(splash) = app.get_webview_window(SPLASH) {
            let _ = splash.close();
        }
        match target.parse() {
            Ok(parsed) => {
                match WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(parsed))
                    .title("Solidway")
                    .inner_size(CONSOLE_SIZE.0, CONSOLE_SIZE.1)
                    .build()
                {
                    Ok(window) => {
                        let _ = window
                            .set_size(tauri::LogicalSize::new(CONSOLE_SIZE.0, CONSOLE_SIZE.1));
                        let _ = window.center();
                        // 可用性 / Dock 图标跟随窗口生命周期：创建恢复，销毁隐藏。销毁统一由
                        // `WindowEvent::Destroyed` 覆盖（用户关窗 / 重启收窗 / 子进程退出收窗），
                        // 不依赖各调用点自觉。
                        let supervisor = Arc::clone(self);
                        let app_handle = app.clone();
                        window.on_window_event(move |event| {
                            if matches!(event, WindowEvent::Destroyed) {
                                supervisor.sync_dock_presence(&app_handle, false);
                            }
                        });
                        self.sync_dock_presence(app, true);
                        // builder 只 orderFront、不激活应用（macOS）；切回 `regular` 后
                        // 再统一置前聚焦，保证「总是显示在最前」。
                        bring_console_to_front(&window);
                    }
                    Err(error) => self.failed(app, format!("打开控制台窗口失败：{error}")),
                }
            }
            Err(error) => self.failed(app, format!("控制台地址非法 {target}：{error}")),
        }
    }

    fn admin_login_now(self: &Arc<Self>, app: &AppHandle) {
        let Some(target) = self.lock().desktop_login_url.clone() else {
            self.failed(
                app,
                "桌面自动登录尚未就绪（未收到 desktop-login 事件）".to_string(),
            );
            return;
        };
        // 已有窗口：直接导航到登录地址（同源导航，cookie 与 sessionStorage 保留）。
        if let Some(window) = app.get_webview_window(MAIN) {
            match target.parse() {
                Ok(url) => {
                    let _ = window.navigate(url);
                    // 导航后同样确保窗口真正前置（可能最小化；macOS 上还需激活应用）。
                    bring_console_to_front(&window);
                }
                Err(error) => self.failed(app, format!("自动登录地址非法 {target}：{error}")),
            }
            return;
        }
        // 没有窗口：走正常打开流程（会使用 desktop-login 地址）。
        self.open_console_now(app);
    }

    /// 读取子进程 stdout：落日志 + 解析事件。
    fn spawn_reader(
        self: &Arc<Self>,
        stdout: ChildStdout,
        app: AppHandle,
        generation: u64,
        log_path: PathBuf,
    ) {
        let supervisor = Arc::clone(self);
        std::thread::spawn(move || {
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .ok();
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // 引导 / 桌面登录事件里是令牌：只进内存，绝不落日志。
                if !is_secret_event(&line) {
                    if let Some(file) = log.as_mut() {
                        let _ = writeln!(file, "{line}");
                    }
                }
                supervisor.handle_event(&app, generation, &line);
            }
        });
    }

    fn handle_event(self: &Arc<Self>, app: &AppHandle, generation: u64, line: &str) {
        let Some(rest) = line.strip_prefix("solidway: ") else {
            return;
        };
        let mut parts = rest.split_whitespace();
        let Some(event) = parts.next() else {
            return;
        };
        let mut fields: HashMap<&str, &str> = HashMap::new();
        for token in parts {
            if let Some((key, value)) = token.split_once('=') {
                fields.insert(key, value);
            }
        }

        // 桌面偏好：与 generation 无关（设置是全局的），直接投递到主线程执行。
        if event == "desktop-apply" {
            let (Some(autostart), Some(close_to_tray)) = (
                flag(fields.get("autostart")),
                flag(fields.get("close_to_tray")),
            ) else {
                return;
            };
            let apply_id = fields.get("id").map(|value| (*value).to_string());
            self.dispatch(app, move |_supervisor, app| {
                if let Some(settings) = app.try_state::<Arc<Settings>>() {
                    // 失败已写回执与壳日志，控制台据此回滚开关。
                    let _ = settings.apply(autostart, close_to_tray, apply_id.as_deref());
                }
            });
            return;
        }

        let restart_requested = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            match event {
                "listening" => {
                    inner.addr = fields.get("addr").map(|value| (*value).to_string());
                    inner.bind_failure = None;
                }
                "bind-failed" => {
                    inner.bind_failure = Some(self.describe_bind_failure(&fields));
                    inner.failure = Some(classify_bind(fields.get("kind").copied().unwrap_or("")));
                }
                "bootstrap" => {
                    inner.bootstrap_url = fields.get("url").map(|value| (*value).to_string());
                }
                "desktop-login" => {
                    inner.desktop_login_url = fields.get("url").map(|value| (*value).to_string());
                }
                "bootstrap-consumed" => {
                    // 引导已完成：后续「打开控制台」改用桌面自动登录地址。
                    inner.bootstrap_url = None;
                }
                "restart-requested" => return self.restart_from_event(app, generation),
                _ => return,
            }
            false
        };
        let _ = restart_requested;
        self.update_ui(app);
    }

    /// `bind-failed` → 可读错误页文案（含占用者与恢复路径）。
    fn describe_bind_failure(&self, fields: &HashMap<&str, &str>) -> String {
        let addr = fields.get("addr").copied().unwrap_or("?");
        let kind = fields.get("kind").copied().unwrap_or("bind-error");
        let mut text = match (fields.get("holder_pid"), fields.get("holder_comm")) {
            (Some(pid), Some(comm)) => format!("端口 {addr} 已被占用（{comm}，pid {pid}）"),
            _ => format!("无法监听 {addr}（{kind}）"),
        };
        text.push_str(&format!(
            "\n\n请释放该端口，或用 --listen 指定其他端口后重启。\n\
             如需在界面里修改端口，可先手动启动：\n  \
             solidway {} --listen 127.0.0.1:<其他端口> --mode standalone\n\n\
             data_root: {}",
            self.config.data_root.display(),
            self.config.data_root.display()
        ));
        text
    }

    /// 界面请求的重启：无条件立即重启，不走退避、不计失败次数。
    fn restart_from_event(self: &Arc<Self>, app: &AppHandle, generation: u64) {
        self.dispatch(app, move |supervisor, app| {
            if supervisor.lock().generation != generation {
                return;
            }
            supervisor.kill_child();
            supervisor.lock().restarts = 0;
            supervisor.start_now(app);
        });
    }

    /// 监控线程：握手超时 / 子进程退出 / 健康探测。
    fn monitor(self: &Arc<Self>, app: AppHandle, generation: u64) {
        let supervisor = Arc::clone(self);
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(800));
            let (current, exited, addr, elapsed) = {
                let mut inner = supervisor.lock();
                if inner.generation != generation {
                    return;
                }
                let exited = match inner.child.as_mut() {
                    Some(child) => !matches!(child.try_wait(), Ok(None)),
                    None => true,
                };
                (
                    inner.generation,
                    exited,
                    inner.addr.clone(),
                    inner.started_at.elapsed(),
                )
            };
            let _ = current;

            if exited {
                supervisor.on_child_exit(&app, generation);
                return;
            }

            match addr {
                Some(addr) => {
                    if config::health_ok(&addr) {
                        let became_running = {
                            let mut inner = supervisor.lock();
                            if inner.state == State::Running {
                                false
                            } else {
                                inner.state = State::Running;
                                true
                            }
                        };
                        if became_running {
                            supervisor.on_running(&app);
                        }
                    }
                }
                None if elapsed > HANDSHAKE_TIMEOUT => {
                    let log = supervisor.config.data_root.join("solidway.log");
                    supervisor.kill_child();
                    supervisor.dispatch(&app, move |supervisor, app| {
                        let tail = log_tail(&supervisor.config.data_root, 20);
                        let mut text = format!(
                            "等待 {} 秒仍未收到 listening 事件。\n日志：{}",
                            HANDSHAKE_TIMEOUT.as_secs(),
                            log.display()
                        );
                        if !tail.is_empty() {
                            text.push_str("\n\n—— 日志末尾 ——\n");
                            text.push_str(&tail);
                        }
                        supervisor.set_state(State::Failed, "启动超时", Some(text));
                        supervisor.lock().failure = Some(FailureKind::Timeout);
                        supervisor.ensure_splash(app);
                        supervisor.update_ui(app);
                    });
                    return;
                }
                None => {}
            }
        });
    }

    fn on_running(self: &Arc<Self>, app: &AppHandle) {
        self.dispatch(app, |supervisor, app| supervisor.on_running_now(app));
    }

    fn on_running_now(self: &Arc<Self>, app: &AppHandle) {
        let bootstrap_url = self.lock().bootstrap_url.clone();

        match bootstrap_url {
            // 引导未完成：直接进设置页，用户无需抄写任何凭证。
            Some(url) => {
                self.set_state(
                    State::Running,
                    "首次启动：请设置 admin 口令",
                    Some(format!(
                        "正在打开设置页：{url}\n\n\
                         设置口令后即进入控制台；引导链接只在本次进程内有效，\
                         重启会重新签发。"
                    )),
                );
                self.update_ui(app);
                // 静默启动不自动弹引导页；托盘「打开控制台」仍指向它。
                if !self.config.hidden {
                    self.open_console_now(app);
                }
            }
            None => {
                self.set_state(State::Running, "运行中", None);
                self.update_ui(app);
                if !self.config.hidden {
                    self.open_console_now(app);
                }
            }
        }
    }

    fn on_child_exit(self: &Arc<Self>, app: &AppHandle, generation: u64) {
        let (retry_in, detail, bind_failure) = {
            let mut inner = self.lock();
            if inner.generation != generation {
                return;
            }
            let status = inner
                .child
                .as_mut()
                .and_then(|child| child.try_wait().ok().flatten());
            inner.child = None;
            let detail = match status {
                Some(status) => format!("solidway 已退出：{status}"),
                None => "solidway 已退出".to_string(),
            };
            let bind_failure = inner.bind_failure.clone();
            let retry = if bind_failure.is_none()
                && self.config.restart_on_failure
                && inner.restarts < MAX_RESTARTS
            {
                inner.restarts += 1;
                Some(Duration::from_secs(1u64 << (inner.restarts - 1).min(5)))
            } else {
                None
            };
            (retry, detail, bind_failure)
        };

        if let Some(delay) = retry_in {
            eprintln!(
                "solidway 异常退出，{}s 后自动重启（{detail}）",
                delay.as_secs()
            );
            let supervisor = Arc::clone(self);
            let handle = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(delay);
                supervisor.start(&handle);
            });
            return;
        }

        self.dispatch(app, move |supervisor, app| {
            if let Some(window) = app.get_webview_window(MAIN) {
                let _ = window.close();
            }
            let log = supervisor.config.data_root.join("solidway.log");
            let (state, status, detail, kind) = match bind_failure {
                Some(text) => {
                    let tail = log_tail(&supervisor.config.data_root, 20);
                    let mut detail = text;
                    if !tail.is_empty() {
                        detail.push_str("\n\n—— 日志末尾 ——\n");
                        detail.push_str(&tail);
                    }
                    (State::Failed, "端口冲突", detail, None)
                }
                None => {
                    let tail = log_tail(&supervisor.config.data_root, 20);
                    let mut text = format!("{detail}\n\n日志：{}", log.display());
                    if !tail.is_empty() {
                        text.push_str("\n\n—— 日志末尾 ——\n");
                        text.push_str(&tail);
                    }
                    (State::Stopped, "Solidway 已停止", text, Some(FailureKind::Crash))
                }
            };
            supervisor.set_state(state, status, Some(detail));
            {
                let mut inner = supervisor.lock();
                if inner.failure.is_none() {
                    inner.failure = kind;
                }
            }
            supervisor.ensure_splash(app);
            supervisor.update_ui(app);
        });
    }

    /// 进入失败态并保证 splash 可见。
    fn failed(self: &Arc<Self>, app: &AppHandle, message: String) {
        self.set_state(State::Failed, "启动失败", Some(message));
        self.ensure_splash(app);
        self.update_ui(app);
    }

    fn set_state(&self, state: State, status: impl Into<String>, detail: Option<String>) {
        let mut inner = self.lock();
        inner.state = state;
        inner.status = status.into();
        inner.detail = detail;
    }

    /// 把当前状态同步到托盘与 splash（仅主线程）。
    fn update_ui(self: &Arc<Self>, app: &AppHandle) {
        let (state, addr) = {
            let inner = self.lock();
            (inner.state, inner.addr.clone())
        };
        let label = state.label(addr.as_deref());
        let _ = self.status_item.set_text(format!("状态：{label}"));
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(format!("Solidway · {label}")));
        }
        if let Some(splash) = app.get_webview_window(SPLASH) {
            self.apply_status_to(&splash);
        }
    }

    fn ensure_splash(self: &Arc<Self>, app: &AppHandle) {
        if app.get_webview_window(SPLASH).is_some() {
            return;
        }
        let supervisor = Arc::clone(self);
        let Ok(window) = WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App("index.html".into()))
            .title("Solidway")
            .inner_size(SPLASH_SIZE.0, SPLASH_SIZE.1)
            .resizable(false)
            .on_page_load(move |window, _payload| supervisor.apply_status_to(&window))
            .build()
        else {
            return;
        };
        let _ = window.set_size(tauri::LogicalSize::new(SPLASH_SIZE.0, SPLASH_SIZE.1));
        let _ = window.center();
        self.apply_status_to(&window);
    }

    /// 控制台主窗口存在性的同步入口：驱动 **macOS Dock 图标** 显隐（activation policy）。
    ///
    /// 托盘「打开控制台」不参与同步：它始终可用，点击后由 `open_action` 按状态分流
    /// （有窗口前置 / 无窗口重建 / 未就绪展示状态页）。置灰在 Linux 托盘上不总是生效，
    /// 且会让主入口在最需要的时候不可点。
    ///
    /// 切换时机只有两个：主窗口创建成功（`true`）与 `WindowEvent::Destroyed`（`false`）；
    /// 启动初始形态由 `main.rs` 以 `false` 调用（与「未开窗 → 隐藏」一致）。
    pub fn sync_dock_presence(&self, app: &AppHandle, present: bool) {
        apply_dock_presence(app, present);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    fn spawn_child(&self, log_path: &Path) -> std::result::Result<Child, (FailureKind, String)> {
        let bin = self
            .bin
            .as_ref()
            .map_err(|error| (FailureKind::MissingBinary, error.clone()))?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .map_err(|error| {
                (
                    FailureKind::SpawnFailed,
                    format!("打开日志 {} 失败：{error}", log_path.display()),
                )
            })?;
        let log_err = log.try_clone().map_err(|error| {
            (FailureKind::SpawnFailed, format!("复制日志句柄失败：{error}"))
        })?;

        let mut command = Command::new(bin);
        command
            .arg(&self.config.data_root)
            .arg("--mode")
            .arg("desktop")
            // stdin 是存活信号：壳死亡时管道关闭，子进程读到 EOF 即退出。
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log_err));

        // Windows：网关是控制台子系统程序（这样独立运行时终端能看到 stdout 事件），
        // 由壳拉起时 MUST NOT 弹出黑色控制台窗口。CREATE_NO_WINDOW (0x0800_0000)
        // 只影响本次派生，不改变二进制本身的子系统类型。
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let child = command
            .spawn()
            .map_err(|error| {
                (
                    classify_spawn_error(error.kind()),
                    format!("启动 {} 失败：{error}", bin.display()),
                )
            })?;

        let _ = log;
        CHILD_PID.store(child.id() as i32, Ordering::SeqCst);
        Ok(child)
    }

    fn kill_child(&self) {
        let mut inner = self.lock();
        inner.generation += 1; // 让在跑的监控线程退出
        inner.addr = None;
        inner.bootstrap_url = None;
        inner.desktop_login_url = None;
        CHILD_PID.store(0, Ordering::SeqCst);
        if let Some(mut child) = inner.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_bind, classify_spawn_error, flag, open_action, strip_ansi, FailureKind, OpenAction, State};

    #[test]
    fn secret_events_are_filtered_from_log() {
        assert!(super::is_secret_event(
            "solidway: bootstrap token=abc url=http://x/bootstrap#abc"
        ));
        assert!(super::is_secret_event(
            "solidway: desktop-login url=http://127.0.0.1:6565/desktop#abc"
        ));
        assert!(!super::is_secret_event("solidway: bootstrap-consumed"));
        assert!(!super::is_secret_event(
            "solidway: listening addr=127.0.0.1:6565"
        ));
    }

    #[test]
    fn bind_failure_classification() {
        assert_eq!(classify_bind("permission-denied"), FailureKind::PortForbidden);
        assert_eq!(classify_bind("address-in-use"), FailureKind::PortConflict);
        assert_eq!(classify_bind(""), FailureKind::PortConflict);
    }

    #[test]
    fn spawn_error_classification() {
        use std::io::ErrorKind;
        assert_eq!(
            classify_spawn_error(ErrorKind::NotFound),
            FailureKind::MissingBinary
        );
        assert_eq!(
            classify_spawn_error(ErrorKind::PermissionDenied),
            FailureKind::NotExecutable
        );
        assert_eq!(
            classify_spawn_error(ErrorKind::ExecutableFileBusy),
            FailureKind::SpawnFailed
        );
    }

    #[test]
    fn failure_copy_is_user_facing() {
        let (title, hint) = FailureKind::MissingBinary.copy();
        assert!(title.contains("安装"), "{title}");
        assert!(hint.contains("重新下载"), "{hint}");
        let (_, hint) = FailureKind::PortConflict.copy();
        assert!(hint.contains("占用"), "{hint}");
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(strip_ansi("\u{1b}[2mabc\u{1b}[0m"), "abc");
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn open_action_keeps_failure_detail() {
        // 有地址：永远直接开窗，与状态无关。
        assert_eq!(open_action(true, State::Failed, true), OpenAction::Open);
        assert_eq!(open_action(true, State::Starting, false), OpenAction::Open);
        // 启动中且还没地址：展示启动页，不判失败。
        assert_eq!(open_action(false, State::Starting, false), OpenAction::ShowSplash);
        // 已有失败 / 停止原因：展示状态页并保留原因（端口冲突、找不到二进制等）。
        assert_eq!(open_action(false, State::Failed, true), OpenAction::ShowFailure);
        assert_eq!(open_action(false, State::Stopped, true), OpenAction::ShowFailure);
        // 既无地址也无原因：才用通用提示判失败。
        assert_eq!(open_action(false, State::Failed, false), OpenAction::Fail);
    }

    #[test]
    fn console_target_prefers_bootstrap_then_desktop_login() {
        assert_eq!(
            super::console_target(
                Some("http://a/bootstrap#t"),
                Some("http://a/desktop#d"),
                Some("a")
            ),
            Some("http://a/bootstrap#t".to_string())
        );
        assert_eq!(
            super::console_target(None, Some("http://a/desktop#d"), Some("a")),
            Some("http://a/desktop#d".to_string())
        );
        assert_eq!(
            super::console_target(None, None, Some("127.0.0.1:6565")),
            Some("http://127.0.0.1:6565/".to_string())
        );
        assert_eq!(super::console_target(None, None, None), None);
    }

    #[test]
    fn flag_accepts_contract_values() {
        assert_eq!(flag(Some(&"1")), Some(true));
        assert_eq!(flag(Some(&"0")), Some(false));
        assert_eq!(flag(Some(&"true")), Some(true));
        assert_eq!(flag(Some(&"false")), Some(false));
        assert_eq!(flag(Some(&"2")), None);
        assert_eq!(flag(None), None);
    }
}
