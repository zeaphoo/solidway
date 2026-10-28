//! Solidway 桌面壳入口：装配监督器 + splash 窗口 + 托盘。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod config;
mod desktop;
mod prefs;
mod supervisor;

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use autostart::TauriBackend;
use config::AppConfig;
use desktop::Settings;
use prefs::{LoadState, Prefs};
use supervisor::{Supervisor, SPLASH_SIZE, TRAY_ID};

fn main() {
    // 被 SIGTERM/SIGINT 中断时同步清理子进程，不留孤儿。
    supervisor::install_signal_handlers();

    tauri::Builder::default()
        // 单实例必须最先注册：第二个实例在派生 `solidway` 之前就退出，
        // 否则「登录自启 + 用户手动双击」必然撞端口。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // 静默启动的第二次启动（登录自启 + 手动自启并发）不抢焦点。
            if argv.iter().any(|arg| arg == "--hidden") {
                return;
            }
            if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                supervisor.open_console(app);
            }
        }))
        // 登录自启固定带 `--hidden`；注册目标与平台细节交给插件。
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            let config = match AppConfig::load(&handle) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("{error}");
                    show_fatal(&handle, &error);
                    return Ok(());
                }
            };
            // 资源目录是打包器放 `solidway` 的位置（客户端包自带网关时走这里）。
            let resource_dir = handle.path().resource_dir().ok();
            let bin = config::locate_solidway(resource_dir.as_deref());

            // 偏好：加载 → 物化（缺失 / 缺字段 / 损坏时写回生效值）。
            // 服务端只读这个文件来渲染控制台，所以文件必须恒等于壳的行为。
            let loaded = Prefs::load(&config.data_root);
            if loaded.state != LoadState::Ok {
                if let Err(error) = loaded.prefs.save(&config.data_root) {
                    desktop::log(&config.data_root, &format!("写入桌面偏好失败：{error}"));
                }
            }

            let settings = Arc::new(Settings::new(
                config.data_root.clone(),
                Box::new(TauriBackend::new(handle.clone())),
                loaded.prefs,
            ));
            app.manage(Arc::clone(&settings));

            // 托盘：状态（只读）+ 打开控制台 / 重启 / 两个偏好开关 / 退出
            let status_item =
                MenuItem::with_id(app, "status", "状态：启动中…", false, None::<&str>)?;
            // 始终可用：点击时按状态分流（有窗口→前置；无窗口→重建；未就绪→状态页）。
            // 不做置灰——Linux 托盘的 set_enabled 不总是生效，且置灰会让主入口失效。
            let open_item = MenuItem::with_id(app, "open", "打开控制台", true, None::<&str>)?;
            let admin_login_item =
                MenuItem::with_id(app, "admin-login", "以管理员自动登录", true, None::<&str>)?;
            let restart_item =
                MenuItem::with_id(app, "restart", "重启 Solidway", true, None::<&str>)?;
            let autostart_item = CheckMenuItem::with_id(
                app,
                "autostart",
                "开机自启动",
                true,
                settings.autostart(),
                None::<&str>,
            )?;
            let close_to_tray_item = CheckMenuItem::with_id(
                app,
                "close-to-tray",
                "关闭窗口时最小化到托盘",
                true,
                settings.close_to_tray(),
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &status_item,
                    &PredefinedMenuItem::separator(app)?,
                    &open_item,
                    &admin_login_item,
                    &restart_item,
                    &PredefinedMenuItem::separator(app)?,
                    &autostart_item,
                    &close_to_tray_item,
                    &PredefinedMenuItem::separator(app)?,
                    &quit_item,
                ],
            )?;
            settings.attach_items(autostart_item.clone(), close_to_tray_item.clone());

            // 启动对账：按偏好重建 / 移除系统登录项（失败只记录，不阻塞启动）。
            settings.reconcile();

            let supervisor = Supervisor::new(config, bin, status_item);
            app.manage(Arc::clone(&supervisor));
            // 启动初始形态：尚无控制台窗口（`--hidden` 与普通启动一致，splash /
            // notice 不计入）→ Dock 图标隐藏（`accessory`）。托盘入口保持可用。
            supervisor.sync_dock_presence(&handle, false);

            let tray_supervisor = Arc::clone(&supervisor);
            let tray_settings = Arc::clone(&settings);
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tauri::include_image!("icons/32x32.png"))
                .tooltip("Solidway")
                .menu(&menu)
                // Windows：左键单击不弹菜单，好让双击事件能到应用（右键仍然出菜单）；
                // 其他平台保持「左键出菜单」的既有习惯。
                .show_menu_on_left_click(!cfg!(target_os = "windows"))
                .on_tray_icon_event(|tray, event| {
                    // Windows：左键单击 = 打开控制台（右键仍然出菜单）。
                    // 同时接受 DoubleClick：两次单击事件本身是幂等的（窗口存在则前置），
                    // 保留它是为了不依赖 NSIS/系统对双击的合成行为。
                    //
                    // 平台差异（tray-icon 的硬限制，不是我们的取舍）：
                    //   - Windows：派发 `Click` / `DoubleClick`；
                    //   - macOS：只有 `Click`，但左键默认出菜单；
                    //   - Linux：gtk/ksni 后端只支持菜单，不派发任何点击事件。
                    #[cfg(target_os = "windows")]
                    if matches!(
                        event,
                        tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } | tauri::tray::TrayIconEvent::DoubleClick {
                            button: tauri::tray::MouseButton::Left,
                            ..
                        }
                    ) {
                        let app = tray.app_handle();
                        if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                            supervisor.open_console(app);
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    let _ = (tray, event);
                })
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => tray_supervisor.open_console(app),
                    "admin-login" => tray_supervisor.admin_login(app),
                    "restart" => tray_supervisor.restart(app),
                    "autostart" => {
                        // 勾选状态由菜单自身翻转，读回即为新意图。
                        let intent = autostart_item.is_checked().unwrap_or(false);
                        if let Err(error) = tray_settings.set_autostart(intent) {
                            let _ = autostart_item.set_checked(!intent);
                            show_notice(app, "开机自启动未生效", &error);
                        }
                    }
                    "close-to-tray" => {
                        let intent = close_to_tray_item.is_checked().unwrap_or(false);
                        if let Err(error) = tray_settings.set_close_to_tray(intent) {
                            let _ = close_to_tray_item.set_checked(!intent);
                            show_notice(app, "关闭行为未生效", &error);
                        }
                    }
                    "quit" => {
                        tray_supervisor.shutdown();
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            supervisor.start(&handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| match event {
            // 关掉最后一个窗口（`code` 为 None）时是否留在托盘由偏好决定；
            // 显式退出（`code` 有值，例如托盘「退出」调用 `app.exit(0)`）必须放行，
            // 否则 `prevent_exit` 会把退出请求一起拦下。
            // 未装配偏好或监督器（配置失败）时不拦截，避免窗口关不掉。
            tauri::RunEvent::ExitRequested { code, api, .. } => {
                let stay = code.is_none()
                    && app
                        .try_state::<Arc<Settings>>()
                        .is_some_and(|settings| settings.close_to_tray())
                    && app.try_state::<Arc<Supervisor>>().is_some();
                if stay {
                    api.prevent_exit();
                }
            }
            tauri::RunEvent::Exit => {
                if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                    supervisor.shutdown();
                }
            }
            // macOS 点击 Dock 图标（`applicationShouldHandleReopen`）：与托盘
            // 「打开控制台」走同一路径——窗口在则 show + focus（不导航、不刷新），
            // 不在则重建。`Reopen` 变体仅 macOS 存在，非 macOS 平台在编译期即无
            // 此分支，因此无需运行时判断平台。
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                if let Some(supervisor) = app.try_state::<Arc<Supervisor>>() {
                    supervisor.open_console(app);
                }
            }
            _ => {}
        });
}

/// 配置或环境不可用时，仍给出一个可读窗口。
fn show_fatal(app: &AppHandle, message: &str) {
    let message = message.to_string();
    let for_load = message.clone();

    let Ok(window) = WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("index.html".into()))
        .title("Solidway")
        .inner_size(SPLASH_SIZE.0, SPLASH_SIZE.1)
        .resizable(false)
        .on_page_load(move |window, _payload| inject_fatal(&window, &for_load))
        .build()
    else {
        return;
    };
    let _ = window.set_size(tauri::LogicalSize::new(SPLASH_SIZE.0, SPLASH_SIZE.1));
    let _ = window.center();
    inject_fatal(&window, &message);
}

/// 把错误文案写进 splash（页面加载完成后再写一次，避免丢失）。
fn inject_fatal(window: &WebviewWindow, message: &str) {
    inject_status(window, "启动失败", Some(message));
}

/// 偏好类操作失败时的可见提示：复用 splash 页，不引入对话框依赖。
fn show_notice(app: &AppHandle, title: &str, detail: &str) {
    const NOTICE: &str = "notice";

    if let Some(window) = app.get_webview_window(NOTICE) {
        let _ = window.show();
        let _ = window.set_focus();
        inject_status(&window, title, Some(detail));
        return;
    }

    let title_for_load = title.to_string();
    let detail_for_load = detail.to_string();
    let Ok(window) = WebviewWindowBuilder::new(app, NOTICE, WebviewUrl::App("index.html".into()))
        .title("Solidway")
        .inner_size(SPLASH_SIZE.0, SPLASH_SIZE.1)
        .resizable(false)
        .on_page_load(move |window, _payload| {
            inject_status(&window, &title_for_load, Some(&detail_for_load))
        })
        .build()
    else {
        return;
    };
    let _ = window.set_size(tauri::LogicalSize::new(SPLASH_SIZE.0, SPLASH_SIZE.1));
    let _ = window.center();
    inject_status(&window, title, Some(detail));
}

/// 注入 `ui/index.html` 的状态行与详情区。
fn inject_status(window: &WebviewWindow, text: &str, detail: Option<&str>) {
    let text = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string());
    let detail = match detail {
        Some(detail) => serde_json::to_string(detail).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    };
    let _ = window.eval(format!(
        "window.solidwaySetStatus && window.solidwaySetStatus({text}, {detail});"
    ));
}
