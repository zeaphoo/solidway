# ADR 0019 — 桌面应用 Bundle ID 迁移 `xyz.solidway.desktop` → `net.quadhub.solidway`

## 状态

已接受（2026-11-10，Charles 裁决）

## 背景

桌面应用的 `identifier`（`app/tauri.conf.json`）同时是 **macOS Bundle ID** 与 Tauri 派生
`app_data_dir()` 的依据，因此它决定了壳与网关子进程共用的 `data_root`
（Linux `~/.local/share/<id>`、macOS `~/Library/Application Support/<id>`、
Windows `%APPDATA%\<id>`）。原值 `xyz.solidway.desktop` 取自域名 `solidway.xyz` 的反向 DNS，
现归属方改用 `quadhub.net`，需要换到 `net.quadhub.solidway` 这一命名空间下。

## 决策

1. **identifier 迁移为 `net.quadhub.solidway`**：只改构建配置一行，命名约定改为
   `<域名倒序>.<项目>`（不再带 `.desktop` 后缀——桌面端已无同域的其他端）。
2. **不写迁移代码，不做数据目录自动搬迁**：`data_root` 仍由 Tauri `app_data_dir()` 派生，
   旧目录（`xyz.solidway.desktop`）保持原样，由使用者按需手工改名或删除；
   文档（`docs/getting-started*.md`、`packaging/README.md`、`scripts/package-windows.sh`）
   同步写明新路径与旧目录处置。
3. **保持「壳与网关共用同一份 `data_root`」的不变式**：网关仍以**唯一位置参数**
   `data_root --mode desktop` 启动（`app/src/supervisor.rs` 的 `child_args` + 单测锁定），
   壳不为其另指数据目录、不传数据目录相关的环境变量；webview 自身的缓存目录不在本 ADR 范围内。

## 被否方案

- **保留 `xyz.solidway.desktop`**：与新的归属命名空间不一致，后续签名 / 公证与分发归属对不上。
- **保留 `.desktop` 后缀（`net.quadhub.solidway.desktop`）**：桌面端只有这一个端，再加一层后缀属冗余。
- **启动时把旧目录 `rename` 到新目录**：跨 identifier 的静默迁移要处理「新旧都存在」「旧目录是
  符号链接」「迁移中途崩溃」等分支，收益仅省一次手工改名；当前无存量正式用户，不值得引入。
- **只改 Bundle ID、继续用旧数据目录（固定路径覆盖）**：等于放弃平台惯例（macOS 容器 / Windows
  ACL 均按 identifier 归属），且需要自建路径解析，增加永久维护面。

## 后果

- macOS Bundle ID 变为 `net.quadhub.solidway`，后续签名 / 公证产物随之使用新 ID。
- 三个平台的数据目录名变为 `net.quadhub.solidway`；旧目录不会自动清理，需手工改名或删除。
- 网关侧无改动：`solidway <data_root>` 是它唯一的数据目录入口，壳继续传同一个 `data_root`。

## 落地

`app/tauri.conf.json`（identifier）+ `docs/getting-started.md` / `docs/getting-started.zh.md` /
`packaging/README.md` / `scripts/package-windows.sh`（数据目录文案）+ `app/src/supervisor.rs`
（`child_args` 与其单测，显式锁定「网关数据目录 = 壳的 `data_root`」）。
