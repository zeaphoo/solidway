# Tasks

## 1. identifier 迁移（配置 + 文档同步）

- [x] 1.1 `app/tauri.conf.json` 的 `identifier` 改为 `xyz.solidway.desktop`。验证：`grep -rn 'xyz\.solidway' --exclude-dir=.git` 结果仅剩 `xyz.solidway.desktop` 与有意保留的历史说明（R1 执行注意：命中归档 change 提案——如 `2026-10-27-app-tray-entry-and-identifier`——与 ADR 0011 修订记录中的 `xyz.solidway` 字样属有意保留的历史留痕，不改）
- [x] 1.2 同步数据目录相关文档/文案：`docs/getting-started.md` / `docs/getting-started.zh.md` 数据目录表、`packaging/README.md` mac 节数据目录（含 `DATA=` 示例与备份命令）、`scripts/package-windows.sh` 卸载说明里的 `%APPDATA%\xyz.solidway`，并在 `packaging/README.md` 加一行开发期旧目录手工迁移说明。验证：三平台数据目录表述全部为 `xyz.solidway.desktop`

## 2. 公证脚本 `scripts/notarize-mac.sh`（design D2/D3）

- [x] 2.1 新增 `scripts/notarize-mac.sh`：zip 提交 `.app` → 等待公证通过（实现形态：`notarytool submit` 拿 submission id → `notarytool wait <id> --timeout 30m`，或外层 `timeout` 包裹；超时即失败）→ 钉票 `.app` + `stapler validate` → 由已钉票 `.app` 重建 `solidway-app_<version>_<arch>.dmg`（命名沿用）→ 钉票 dmg → 挂载断言（内含 `solidway`）→ `spctl -a -vv` 验收输出含 `accepted source=Notarized Developer ID`。验证：`bash -n scripts/notarize-mac.sh` 通过；缺凭据 env 时以可读错误退出并列出所需变量（`MAC_NOTARY_API_KEY_ID` / `MAC_NOTARY_API_ISSUER_ID` / `MAC_NOTARY_API_KEY_P8_BASE64`）
- [x] 2.2 `scripts/package-mac.sh` 零改动回归。验证：本机执行 `scripts/package-mac.sh`（自签路径）行为与改动前一致，产出未公证 dmg（应急路径可用）

## 3. release.yml mac job 接入（design D1/D5/D7）

- [x] 3.1 secrets 校验扩展到 6 项（`MAC_SIGN_IDENTITY` 必填，移除 env 默认值 `AKSL Software`），失败文案输出完整 Secrets 名称清单与配置指引；验证：`actionlint` 0 告警，缺任一 secrets 时 job 明确失败且不产出 mac 产物
- [x] 3.2 新增公证步骤：调用 `scripts/notarize-mac.sh`（`.p8` 解码到 `$RUNNER_TEMP` 临时文件，`always()` 清理；公证失败 = job 失败，不上传）。验证：`actionlint` 0 告警，步骤顺序为「构建签名 → 自检 → 公证/钉票 → 上传 dmg」
- [x] 3.3 同步分发说明文案：`packaging/README.md` / `docs/getting-started.md` / `docs/getting-started.zh.md` 的 mac 分发说明从「自签、未公证、需手动放行（xattr 兜底）」改为「Developer ID 签名 + 已公证」。验证：仓库内不再有指导用户 `xattr` 放行 mac 产物的正文

## 4. 演练与验收（合并前实跑）

- [ ] 4.1 确认 Charles 已在 GitHub 仓 Settings 配置 6 个 Secrets（清单见 design.md D1）。验证：mac job 的 secrets 校验步骤通过
- [ ] 4.2 `workflow_dispatch` 演练（`publish=false`、`with_mac=true`）产出已公证 dmg。验证：两个架构产物齐全；`spctl -a -vv` 输出 `accepted source=Notarized Developer ID`；`stapler validate` 通过；演练不创建 Release
- [ ] 4.3 Charles 手工验收。验证：下载演练产物后双击直接打开、无 Gatekeeper 拦截（含断网场景，`.app` 双钉票）

## 5. 决策留痕（随提案已落地）

- [x] 5.1 `docs/adr/0011-mac-desktop-packaging-and-signing.md` 追加「2026-10-29 — Developer ID 签名 + Apple 公证」修订记录（含被否方案与旧「私钥不上云」被取代始末）——提案阶段已落地（grilling Q4=A）。验证：修订记录与 design.md D1/D2/D5/D7 裁决一致

## Workflow follow-up

- 归档顺序依赖（design D4）：**先归档 `2026-10-26-release-pipeline-v-tags`**（`release-pipeline` 主规格落地）再归档本 change，否则归档会拒绝本 delta 的 `MODIFIED`。
- 归档后执行 `openspec validate --all --strict` 确认合并结果。
- 正式发布走既有流程：`v*` 标签触发（版本号人工 bump）。
