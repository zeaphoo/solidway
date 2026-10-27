# release-pipeline-v-tags

## Why

当前「编译 Linux / macOS 客户端」全靠本机手工操作，且分发物不统一：

| 产物 | 现在怎么来 |
|---|---|
| Linux 服务包（deb/rpm + systemd） | `scripts/package-linux.sh`；CI 每次 push 构建，但**只做 workflow artifact**，不发 release |
| Linux 桌面客户端（Tauri deb/AppImage） | 无脚本，README 里一句 `cd app && cargo tauri build --bundles deb,rpm` |
| macOS 客户端（`.app` + `.dmg`，AKSL 自签） | 仅本机 `scripts/package-mac.sh`（ADR 0011 方案 B：CI 不构建 mac 分发物） |

结果：出一次版本要三台机器、靠人记步骤，版本号漂移（tag 与 `Cargo.toml` / `tauri.conf.json` 不一致）没有闸门，产物也没有统一的可下载位置。

ADR 0011 的「修订记录」已经预留了恢复路径：**负责人决定后改为 tag 触发 + 签名 secrets**。本次即按该路径落地，把「打 `v*` tag → CI 出全部客户端与服务包 → 挂到 GitHub Release」做成一件事。

## What Changes

1. 新增 `.github/workflows/release.yml`，触发条件只有 **`push: tags: ['v*']`**（外加手动 `workflow_dispatch` 用于演练，默认不发 release）。
2. 新增 `scripts/check-release-version.sh`：校验 tag 与 `app/tauri.conf.json` 及全部 crates 的版本一致（不一致即失败），版本号作为后续 job 的输出。
3. Linux 服务包 job：复用 `scripts/package-linux.sh` 产出 deb / rpm。
4. Linux 桌面客户端 job：新增 `scripts/package-linux-app.sh`，用 Tauri bundler 产出 `deb` + `AppImage`；运行时通过 PATH 使用服务包安装的 `/usr/bin/solidway`（app 自身不内嵌网关二进制）。
5. Windows 桌面客户端 job：新增 `scripts/package-windows.sh`，产出 NSIS / MSI 安装包 + 便携 zip；安装包通过 `app/tauri.windows.conf.json` 的 `bundle.resources` 把 `solidway.exe` 注入安装目录（与 `Solidway.exe` 同目录），便携 zip 直接放两个 exe。
6. macOS job（matrix：`macos-13` x86_64 / `macos-14` arm64）：从 secrets 导入 AKSL p12 到临时 keychain → `scripts/package-mac.sh` → `codesign --verify --deep --strict` 自检 → 只上传 `.dmg`。secrets 缺失即失败（不降级、不打 ad-hoc 包）。
7. release job：tag 触发时创建/更新 GitHub Release，挂载上述全部产物并生成 release notes；`workflow_dispatch` 默认只上传 artifacts。
8. 文档与 ADR：ADR 0011 增补「2026-10-26 恢复 CI 打包」修订记录；新增 ADR 0018 记录发布流水线口径；`packaging/README.md` 与 `docs/tech-context.md` 同步（含所需 secrets 清单）。

不修改：应用运行时行为、`scripts/package-mac.sh` 与 `scripts/package-linux.sh` 的内部顺序、签名口径（AKSL Software + hardened runtime、禁 ad-hoc、不公证）、现有 `ci.yml`（保持每次 push 的构建/测试职责）。

## 范围外

- Windows 代码签名（无证书；产物未签名，SmartScreen 可能告警，README 说明手动放行）。
- Windows 服务形态（无服务包；网关只随桌面客户端分发，或用户自行放置 `solidway.exe`）。
- Apple 公证 / `spctl` 通过、Homebrew cask、自动更新（Sparkle / Tauri updater）。
- Docker 镜像、apt/rpm 仓库托管、签名密钥托管服务（Vault / KMS）。
- Linux arm64 客户端与 macOS universal 单包（矩阵先各平台原生架构；需要时再加 runner）。
- 版本号自动 bump（tag 仍是人工创建，只做一致性校验）。

## 验证要求

- `python -c "import yaml,glob; [yaml.safe_load(open(p)) for p in glob.glob('.github/workflows/*.yml')]"` 通过（YAML 合法）。
- `shellcheck scripts/*.sh` 无 error（若本机可用）。
- `./scripts/check-release-version.sh` 输出当前版本；`./scripts/check-release-version.sh v0.1.0` 通过；`v0.2.0` 与非 `v` 前缀输入必须失败。
- 首次真实 `v*` tag 需人工冒烟：release 页面产物齐全、mac dmg 的 `codesign --verify --deep --strict` 通过、Linux AppImage 可启动并找到 `solidway`。
