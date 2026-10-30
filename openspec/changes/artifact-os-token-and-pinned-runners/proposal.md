# Proposal: artifact-os-token-and-pinned-runners

## Why

两个问题都来自 v0.5.0-rc.1 的实际发布：

1. **产物名无法一眼分辨平台**：Release 资产列表只有文件名可看，当前命名 `solidway-app_<ver>_<arch>.<ext>` 把平台信息藏在扩展名里（`.dmg` / `.AppImage` / `.exe` / `.deb`），Linux 服务包与 Linux 客户端包只差前缀；带 locale 后缀的 MSI 更是一长串。用户反馈希望在文件名里直接带操作系统名。
2. **构建机版本漂移抬高 glibc 下限**：Linux job 用 `runs-on: ubuntu-latest`。GitHub 已通知 `ubuntu-latest` 将于 2026-10-19 起迁移到 Ubuntu 26；Linux 二进制动态链接 glibc，构建机升级会抬高产物要求的最低 glibc，导致旧发行版用户装不上——且这种变化发生在「重新打同一个 tag」时，不可控。`windows-latest` 同样存在镜像漂移（MSVC / 工具链版本）问题。

## What Changes

- **产物名加 OS 标记**：统一为 `solidway[-app]_<ver>_<os>_<arch>[.<ext>]`（服务包保留 `-<rev>`），`<os>` ∈ `linux` / `windows` / `macos`：
  - Linux 服务：`solidway_<ver>-1_linux_<arch>.deb`、`solidway-<ver>-1.linux.<arch>.rpm`
  - Linux 客户端：`solidway-app_<ver>_linux_<arch>.deb`、`solidway-app_<ver>_linux_<arch>.AppImage`
  - Windows 客户端：`solidway-app_<ver>_windows_x64-setup.exe`、`solidway-app_<ver>_windows_x64_<locale>.msi`、`solidway-app_<ver>_windows_x64_portable.zip`
  - macOS 客户端：`solidway-app_<ver>_macos_<arch>.dmg`
  - 实现：四个打包脚本在产出后重命名（Tauri / cargo-deb / cargo-generate-rpm 的默认名不可配置），包内元数据不变。
- **固定构建机**：`release.yml` 与 `ci.yml` 的 Ubuntu job 改为 `runs-on: ubuntu-22.04`（glibc 2.35，低于 24.04 的 2.39，兼容更老的发行版），Windows job 改为 `runs-on: windows-2022`；MUST NOT 使用 `ubuntu-latest` / `windows-latest`。基线如需变更，显式改 runner（Tauri v2 需要 `libwebkit2gtk-4.1-dev`，22.04 的 universe 源已提供）。
- 文档：`packaging/README.md` 产物表、命名约定、安装示例、glibc 基线说明同步。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `release-pipeline`：
  - MODIFIED「发布产物集」：文件名 MUST 含 OS 标记；服务包与客户端包的命名模板更新。
  - ADDED「构建环境固定」：三平台构建 / 测试 job 均 MUST 固定到具体 runner 版本，MUST NOT 使用 `-latest`。

## Impact

- 脚本：`scripts/package-linux.sh`、`scripts/package-linux-app.sh`、`scripts/package-mac.sh`、`scripts/package-windows.sh`（产物重命名）。
- CI：`.github/workflows/release.yml`、`.github/workflows/ci.yml`（`ubuntu-22.04` / `windows-2022` / `macos-14`，全部固定）。
- 文档：`packaging/README.md`、`CHANGELOG.md`。
- 兼容性：已发布的 `v0.5.0-rc.1` 资产名保持旧格式（不改写历史发布）；新命名从下一个版本生效。
