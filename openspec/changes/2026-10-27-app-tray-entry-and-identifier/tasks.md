# Tasks — app-tray-entry-and-identifier

- [x] 1.1 `app/src/main.rs`：菜单项始终可用；`Supervisor::new` 去掉 `open_item`；初始调用改为 `sync_dock_presence`
- [x] 1.2 `app/src/supervisor.rs`：删除 `open_item` 字段与 `set_enabled` 逻辑；`sync_main_window_presence` → `sync_dock_presence`（只驱动 Dock）
- [x] 1.3 `app/tauri.conf.json`：`identifier` 改为 `xyz.solidway`
- [x] 1.4 不做数据目录迁移（未发布）；`packaging/README.md` 注明老目录自行处理
- [x] 1.5 打包验证：deb（`Package: solidway-app`，含 `usr/lib/Solidway/solidway`）与 AppImage 均正常
- [x] 1.6 `cargo test --manifest-path app/Cargo.toml` 17/17
- [x] 1.7 托盘双击打开控制台（Windows 派发 `DoubleClick`；`show_menu_on_left_click(false)` 让双击能被收到，右键仍出菜单）
- [x] 1.8 平台差异写清：macOS 无 DoubleClick（保持左键菜单）、Linux 后端不派发点击（只能走菜单）
- [x] 1.9 交叉编译校验：`cargo check --target i686-pc-windows-gnu` 通过（验证 cfg(windows) 分支）
