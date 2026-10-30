# app-tray-entry-and-identifier

## Why

Linux 实测两个问题：

1. **托盘「打开控制台」始终灰色不可点**（无论控制台窗口是否已打开）。原设计要求「窗口不存在时置灰」，但 Linux 托盘（AppIndicator/libappindicator 后端）对 `set_enabled` 的传播不可靠，且置灰把唯一的主入口锁死——用户只能去点「以管理员自动登录」，行为与预期不符。
2. **identifier 与自有域名不一致**：`com.solidway.app` 既不匹配自有域名 `solidway.xyz`，又因为以 `.app` 结尾触发 Tauri 告警（macOS 的 bundle 扩展名冲突）。

## What Changes

1. 托盘「打开控制台」改为**始终可用**：点击后按状态分流（窗口存在 → 前置；不存在且有地址 → 重建；尚无地址 → 启动页 / 状态页 / 失败提示，见 `2026-10-27-app-open-console-keeps-detail`）。MUST NOT 再依赖 `MenuItem::set_enabled`。
   - Dock 图标显隐逻辑不变，仍以「控制台主窗口存在性」为唯一判据。
2. `identifier` 改为 **`xyz.solidway`**（`solidway.xyz` 反向 DNS）。
3. **不做数据目录迁移**：应用尚未发布，没有历史用户；README 注明本机老目录（`com.solidway.app`）自行改名或删除。

不修改：`open_action` 的分流语义、Dock 图标行为、`data_root` 解析方式（仍是平台应用数据目录）。

## 验证要求

- `cargo test --manifest-path app/Cargo.toml` 通过（17 条，含 `open_action` 与 `locate_in`）。
- `SKIP_WEB_BUILD=1 ./scripts/package-linux-app.sh` 成功，产物 `solidway-app_<ver>_<arch>.deb` 与 AppImage 均含 `usr/lib/Solidway/solidway`。
- 手工：窗口关闭状态下点击托盘「打开控制台」可重建窗口；窗口已存在时点击只前置不刷新。
