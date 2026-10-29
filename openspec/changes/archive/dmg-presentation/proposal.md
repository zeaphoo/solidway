# Proposal

## Why

mac 客户端首装验收通过（无 Gatekeeper 拦截），但 DMG 安装窗口是 `hdiutil` 默认裸窗口：无 `/Applications` 快捷方式、无图标布局，用户需自行开 Finder 拖拽，安装体验不完整。自定义 dmg 背景方案经编排者 macOS 26 实证被 Finder 忽略（图片与纯色均不渲染），改为交付干净白窗。同时 Charles 已删除本机 AKSL Software 自签证书及其 CA，`scripts/package-mac.sh` 的内置 `AKSL Software` 默认身份与文档中的自签应急口径已失真，存在「本地默认身份静默出自签包」的隐患。

## What Changes

- **dmg 安装盘布局**：卷内 `Solidway.app` 旁提供 `/Applications` 符号链接；固定布局（窗口 660×420，app 居左、Applications 居右，隐藏无关元素）；窗口背景使用 Finder 默认呈现（macOS 26 忽略 `.DS_Store` 的 backgroundImageAlias/backgroundColor，打包链不写背景键）。
- **`.DS_Store` 无头生成**：python3 + `ds_store` 库（vendor 入库带 LICENSE）脚本化进打包链；只写窗口 `bwsp`、图标 `Iloc` 与 `icvp.arrangeBy`，不写背景键；不依赖 Finder/AppleScript，CI 无头环境可生成。
- **共享 dmg 组装**：新增 `scripts/dmg-layout.sh`（staging + `.DS_Store` + `hdiutil` 建盘），`package-mac.sh` 的建盘与 `notarize-mac.sh` 的重建 dmg 共用同一实现——CI 发布的 dmg（公证链重建产物）与本机 dmg 呈现一致。签名/公证/钉票/验收语义零变化（「不碰签名/公证链」按语义级理解，grilling Q1 裁决确认）。
- **本机构建身份解析**（范围追加①）：`package-mac.sh` 移除内置 `AKSL Software` 默认值；`MAC_SIGN_IDENTITY` 显式设置优先（校验存在于钥匙串），未设置时自动探测钥匙串中唯一的 `Developer ID Application` 身份；多身份或探测不到 MUST 失败并给出设置 `MAC_SIGN_IDENTITY` 的明确指引。
- **Copyright**：`app/tauri.conf.json` copyright 字段改为 `Copyright © 2026 solidway.xyz`。
- **文档重写**（范围追加②）：`packaging/README.md` 本机构建节——Developer ID 本机构建姿势（自动探测 + 显式覆盖）、自签应急路径随 AKSL 证书退役的说明（如需自签按应急段重建指引）、CI 依赖 Secrets 不变；不再有背景素材。
- **ADR 留痕**：`docs/adr/0011-mac-desktop-packaging-and-signing.md` 追加修订记录（本机默认自签身份退役，2026-10-29「自签路径保留为应急」口径收敛为「应急 = 显式指定自签身份」）。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `release-pipeline`: MODIFIED「macOS 公证与钉票」（dmg 重建纳入安装盘布局组装、`package-mac.sh`/`notarize-mac.sh` 共用组装实现；原「本机构建链 MUST 保持不变（自签应急路径不受影响）」条款被取代）；ADDED「macOS dmg 安装盘呈现」「本机构建身份解析」「应用版权信息」。该能力已在主 specs（`openspec/specs/release-pipeline/`），无跨 change 归档依赖。

## Impact

- 脚本：新增 `scripts/dmg-layout.sh`、`scripts/python/gen_dmg_ds_store.py`、vendored `scripts/python/ds_store/` + `scripts/python/mac_alias/`（含 LICENSE）；`scripts/package-mac.sh`（身份解析、建盘段改调共享组装、挂载断言追加布局断言）；`scripts/notarize-mac.sh`（仅 dmg 重建行改调共享组装，挂载断言与验收语义不变）。
- 配置：`app/tauri.conf.json`（copyright 一行）。
- 文档：`packaging/README.md`；`docs/adr/0011-mac-desktop-packaging-and-signing.md`（修订记录）。
- **2026-10-30 方向变更**：放弃自定义 dmg 背景（macOS 26 实证忽略图片/纯色背景），移除 `packaging/dmg/` 素材，交付干净白窗；spec delta 的呈现需求同步收窄。
- 不动：`.github/workflows/release.yml`（CI 依赖 Secrets 不变，预期零改动）、签名/公证/钉票/验收语义、Linux/Windows 构建与产物命名。
