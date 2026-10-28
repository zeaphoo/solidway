# Tasks — release-pipeline-v-tags

> 对应提案与设计：`openspec/changes/2026-10-26-release-pipeline-v-tags/`。
> 新增 CI/脚本/文档；不改运行时行为。

## 1. 版本闸门

- [x] 1.1 `scripts/check-release-version.sh`：读 `app/tauri.conf.json` 版本，比对 `crates/{solidway,solidway_api,solidway_core,solidway_egress,solidway_transcode}` 与 `app/`
- [x] 1.2 支持可选参数 `v<version>`：不一致即失败；非 `v` 前缀失败；成功输出裸版本
- [x] 1.3 本机验证：无参 → `0.1.0`；`v0.1.0` → 通过；`v0.2.0` / `0.1.0` → 失败

## 2. Linux 桌面客户端脚本

- [x] 2.1 `scripts/package-linux-app.sh`：依赖检查 → 构建 `web/dist`（可 `SKIP_WEB_BUILD=1`）→ 构建 `solidway` 并临时注入 `app/solidway`（`bundle.resources`）→ `cd app && cargo tauri build --bundles deb,appimage` → 收集到 `dist/linux-app/`
- [x] 2.1.1 deb 包名重写为 `solidway-app`（与服务包 `solidway` 区分），文件名同步
- [x] 2.2 产物命名/断言：至少一个 `.deb` 与一个 `.AppImage`，否则失败
- [x] 2.3 复用 `package-mac.sh` 的版本一致性错误提示风格（中文、指到文件）

## 2b. Windows 客户端脚本

- [x] 2b.1 打包期 `--config` 注入 `bundle.resources`，把 `solidway.exe` 装进应用同目录
- [x] 2b.2 `scripts/package-windows.sh`：Git Bash 校验 → 版本闸门 → 构建 `web/dist`（可跳过）→ `cargo build --release -p solidway` → 注入 `app/solidway.exe` → `cargo tauri build --bundles ${TAURI_BUNDLES:-nsis,msi}` → 收产物到 `dist/windows/`
- [x] 2b.3 便携 zip：`SolidwayApp.exe` + `solidway.exe` + `README.txt`（WebView2 前提、SmartScreen 说明），PowerShell `Compress-Archive`
- [x] 2b.4 `.gitignore` 忽略临时注入的 `app/solidway.exe`

## 3. 发布工作流

- [x] 3.1 `.github/workflows/release.yml`：`on.push.tags: ['v*']` + `workflow_dispatch`（`publish` / `with_mac` 布尔输入）
- [x] 3.2 `permissions: contents: write`；`concurrency: release-<ref>` + `cancel-in-progress: false`
- [x] 3.3 `verify` job：调用 1.1 脚本，`version` 作为 output
- [x] 3.4 `linux-service` job：rust + pnpm + cargo-deb/cargo-generate-rpm → `scripts/package-linux.sh` → upload `dist/linux/*.deb`, `*.rpm`
- [x] 3.5 `linux-app` job：装 Tauri 2 Linux 依赖 → `scripts/package-linux-app.sh` → upload `dist/linux-app/*`
- [x] 3.6 `mac` job：matrix `macos-13` / `macos-14`；secrets 检查（缺失即 `::error::`）→ 临时 keychain 导入 → `cargo install tauri-cli` → `scripts/package-mac.sh` → `codesign --verify --deep --strict` → upload `dist/mac/*.dmg` → `always()` 删除 keychain
- [x] 3.7 `windows-app` job：windows-latest + tauri-cli + `./scripts/package-windows.sh`（`shell: bash`）→ upload `dist/windows/*`
- [x] 3.8 `release` job：`needs` 全部 build job；按事件/输入条件决定是否创建 Release；下载 artifacts 到 `dist/`；产物收集含 `exe/msi/zip`；`softprops/action-gh-release@v2` 带 `generate_release_notes` + `fail_on_unmatched_files`
- [x] 3.9 所有 upload 步骤 `if-no-files-found: error`
- [x] 3.10 `ci.yml` 增加 `windows-app-check`（windows-latest 上 `cargo check --manifest-path app/Cargo.toml`），避免到打 tag 才发现 Windows 编译不过

## 4. 文档与决策记录

- [x] 4.1 `docs/adr/0011-mac-desktop-packaging-and-signing.md` 增补「2026-10-26 恢复 CI 打包（tag 触发 + secrets）」修订记录，指向本 change
- [x] 4.2 `docs/adr/0018-release-pipeline.md`：发布触发点、产物集、签名前提、被否方案
- [x] 4.3 `packaging/README.md`：新增「GitHub Release 产物」小节（secrets 清单、tag 流程、Linux 客户端需先装服务包、mac 自签的 Gatekeeper 说明不变）
- [x] 4.4 `docs/tech-context.md` 发布流水线段改为「tag 触发 CI 出全部产物」
- [x] 4.5 `docs/system-patterns.md` 仓库组织段补「发布只由 `v*` tag 触发」

## 5. 验证

- [x] 5.1 YAML 合法（Python `yaml.safe_load` 全量 workflows）
- [x] 5.2 `shellcheck scripts/package-linux-app.sh scripts/check-release-version.sh`（若可用）
- [x] 5.3 `openspec validate 2026-10-26-release-pipeline-v-tags --type change` 通过
- [x] 5.4 首次 `workflow_dispatch` 演练通过（`publish=false`）后，再打首个 `v*` tag —— **待在仓库侧执行**（需要先在 GitHub 配置 macOS 签名 secrets），本次仅完成流水线与本地闸门验证
