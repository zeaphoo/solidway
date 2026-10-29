# Proposal

## Why

mac 桌面客户端当前以 AKSL 自签分发、不做 Apple 公证：经 quarantine 的用户（浏览器下载）会被 Gatekeeper 拦截、需手动放行，`spctl` 不通过（ADR 0011 当时即约定「对外分发真发生时另立事项做 Developer ID + 公证」）。Apple 开发者个人账号与 `Developer ID Application` 证书（G2 Sub-CA，2031 到期）已就绪，正式分发时机已到：release CI 必须产出签名 + 公证的 dmg，让用户下载后双击直接打开、无 Gatekeeper 拦截。

## What Changes

- **identifier 迁移**：`app/tauri.conf.json` 的 `identifier` 由 `xyz.solidway` 改为 `xyz.solidway.desktop`（Bundle ID 已注册，命名约定 `xyz.<项目>.<端>`）。连带：桌面壳 `data_root` 由 Tauri `app_data_dir()`（按 identifier 派生）决定，三平台数据目录名同步变为 `xyz.solidway.desktop`；`docs/getting-started.md` / `docs/getting-started.zh.md` / `packaging/README.md` / `scripts/package-windows.sh` 卸载说明中的旧目录名一并更新。v0.1.0 未发布，无数据迁移负担。
- **签名口径升级**：mac 发布物由 AKSL 自签改为 **Developer ID Application** 签名 + hardened runtime（`codesign --options runtime` 口径不变）；`MAC_SIGN_IDENTITY` 由「可选、默认 `AKSL Software`」改为**必填**（个人账号身份名无法硬编码）。
- **公证链路接入 release CI**：`.github/workflows/release.yml` 的 mac job 在签名后新增公证步骤（新增独立脚本 `scripts/notarize-mac.sh`，CI 与本机共用）：提交 `.app`（zip）→ `notarytool` 等待通过 → 钉票 `.app` → 从已钉票 `.app` 重建 dmg → 钉票 dmg → `spctl -a -vv` 验证输出 `accepted source=Notarized Developer ID`。`scripts/package-mac.sh` 本机链核心一字不动。
- **公证失败 = 发布失败**：不跳过、不降级为仅签名发布；公证未过或验证不通过的 dmg MUST NOT 上传/发布。
- **取消自签回落**：不再保留临时自签回落；secrets 缺失时 mac job 明确失败并输出 Secrets 配置指引（名称清单）。本机 `scripts/package-mac.sh` 自签路径保留作应急，不受影响。
- **Secrets 约定**（只定名称与读取方式，真实凭据永不入仓库、不入 workflow 明文；由 Charles 在 GitHub 仓 Settings 配置）：沿用 `MAC_CERT_P12_BASE64` / `MAC_CERT_PASSWORD`（内容换成 Developer ID 证书 p12），`MAC_SIGN_IDENTITY` 改必填，新增公证凭据 `MAC_NOTARY_API_KEY_ID` / `MAC_NOTARY_API_ISSUER_ID` / `MAC_NOTARY_API_KEY_P8_BASE64`（App Store Connect API key 形态，grilling Q1 裁决 B）。
- **ADR 留痕**：修订 `docs/adr/0011-mac-desktop-packaging-and-signing.md`（追加修订记录）：记录「CI 构建 + Developer ID 签名 + 公证，p12 与公证凭证进 GitHub Secrets」决策，说明旧「CI 不构建 mac 包、私钥不上云」与「自签不公证」方案为何被取代（grilling Q4 裁决 A，不新建 ADR）。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `release-pipeline`: 「发布产物集」macOS 条款的签名口径由 AKSL 自签改为 Developer ID + Apple 公证；「macOS 构建的签名前提」的 secrets 清单与 `MAC_SIGN_IDENTITY` 必填口径变更；ADDED 新要求覆盖公证提交物、双钉票（`.app` / dmg）与 Gatekeeper 验收闸门。
  注意：`release-pipeline` 能力由 in-flight change `2026-10-26-release-pipeline-v-tags`（30/30 已完工、尚未归档进 `openspec/specs/`）建立，本 change 的 delta 与其在归档时顺序合并（grilling Q3 裁决 A）。

## Impact

- 配置：`app/tauri.conf.json`（identifier 一行）。
- CI：`.github/workflows/release.yml` mac job（secrets 校验与文案、公证步骤、Gatekeeper 验证、`MAC_SIGN_IDENTITY` 必填化）。
- 脚本：新增 `scripts/notarize-mac.sh`；`scripts/package-mac.sh` 不改；`scripts/package-windows.sh` 仅改卸载说明文案。
- 文档：`docs/getting-started.md` / `docs/getting-started.zh.md` / `packaging/README.md`（数据目录名、mac 分发说明从「自签、未公证」改为「Developer ID 签名 + 已公证」）；`docs/adr/0011-mac-desktop-packaging-and-signing.md` 修订记录。
- 运维：GitHub 仓新增 3 个 Secrets、1 个 Secrets 改必填（Charles 操作，清单见 design.md）。
- 不动：Linux/Windows 构建任务结构与产物命名约定、app 运行时行为、`ci.yml`。
