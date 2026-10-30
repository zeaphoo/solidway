# release-pipeline Specification (delta: artifact-os-token-and-pinned-runners)

## MODIFIED Requirements

### Requirement: 发布产物集

发布工作流 SHALL 产出并挂载以下产物：

- Linux 服务包：`.deb` 与 `.rpm`（含 systemd unit），由 `scripts/package-linux.sh` 构建；
- Linux 桌面客户端：`.deb` 与 `.AppImage`，由 Tauri bundler 构建，MUST 自带 `solidway` 网关二进制（`bundle.resources`，落点为打包器资源目录）；`.deb` 的包名 MUST 与服务包 `solidway` 不同（使用 `solidway-app`），避免 dpkg 视为同名包互相替换；
- Windows 桌面客户端：NSIS（或 MSI）安装包 MUST 把 `solidway.exe` 安装到与应用可执行文件同一目录；另 MUST 提供便携 zip（`SolidwayApp.exe` + `solidway.exe` 同目录）。NSIS 安装器 MUST 提供简体中文与英文，并 MUST 在系统语言未命中时提供语言选择；MSI 产物 MUST 按语言分别输出（文件名带 locale 后缀）。

两种安装器 MUST 安装到同一路径（`C:\Program Files\<产品名>`）：NSIS MUST 使用 per-machine 安装模式（其默认 currentUser 会装到 `%LOCALAPPDATA%`，与 MSI 不一致）。应用主程序名 MUST 在忽略大小写后与 `solidway` 不同（Windows/macOS 文件系统不区分大小写），打包脚本 MUST 断言两个可执行文件都存在。Windows 产物 MAY 未做代码签名，但 MUST 在分发说明中标注 SmartScreen 影响与 WebView2 前提；
- macOS 桌面客户端：`.dmg`，由 `scripts/package-mac.sh` 构建、`scripts/notarize-mac.sh` 公证，签名口径 MUST 为 Developer ID Application 签名 + hardened runtime，发布物 MUST 为已公证产物（`.app` 与 dmg 均钉票）。

客户端产物文件名 MUST 统一以 `solidway-app_` 为前缀，服务包 MUST 使用 `solidway_` 前缀，二者 MUST NOT 混淆。文件名 MUST 含版本、**操作系统标记**与架构（服务包另含 `-<rev>`）：客户端为 `solidway-app_<ver>_<os>_<arch>[.<ext>]`，服务包为 `solidway_<ver>-<rev>_<os>_<arch>`；`<os>` MUST 取 `linux` / `windows` / `macos`，架构名 MUST 遵循各平台惯例（Linux `amd64`/`arm64`、Windows `x64`、macOS `x86_64`/`arm64`）。OS 标记 MUST 由打包脚本在产出后落实到最终文件名（包内元数据 MUST NOT 因重命名而改变）。

任一产物缺失时发布任务 MUST 失败（MUST NOT 发布不完整产物集）。

#### Scenario: 产物齐全
- **WHEN** 所有构建任务成功
- **THEN** Release 页面包含 Linux 服务包、Linux 客户端包、Windows 安装包与便携 zip、macOS dmg

#### Scenario: 客户端与服务包文件名可区分
- **WHEN** 下载 Releases 页面的全部产物
- **THEN** 客户端产物均为 `solidway-app_<ver>_<os>_<arch>[.<ext>]`，服务包均为 `solidway_<ver>-<rev>_<os>_<arch>`，仅看文件名即可区分用途

#### Scenario: 文件名带操作系统标记
- **WHEN** 浏览 Releases 页面的资产列表
- **THEN** 每个文件名含 `linux` / `windows` / `macos` 之一，无需依赖扩展名判断平台

#### Scenario: Linux 客户端单装可用
- **WHEN** 用户只安装 Linux 桌面客户端（未安装服务包，PATH 中没有 `solidway`）
- **THEN** 客户端从自带资源中启动网关，控制台可正常打开

#### Scenario: Windows 安装目录可自洽运行
- **WHEN** 用户安装 Windows 安装包或解压便携 zip
- **THEN** `SolidwayApp.exe` 与 `solidway.exe` 位于同一目录，应用无需额外安装服务即可启动网关

#### Scenario: Windows 产物未签名
- **WHEN** 用户首次运行 Windows 产物
- **THEN** 分发说明 MUST 已提示 SmartScreen 可能告警以及 WebView2 运行时前提

#### Scenario: 产物缺失
- **WHEN** 任一构建任务失败或未产出预期文件
- **THEN** Release 不被创建/更新

## ADDED Requirements

### Requirement: 构建环境固定

发布工作流的构建 job MUST 固定到具体 runner 版本：Linux `ubuntu-22.04`、Windows `windows-2022`、macOS arm64 `macos-14`、macOS x86_64 `macos-15-intel`；MUST NOT 使用 `ubuntu-latest` / `windows-latest` / `macos-latest`（`ci.yml` 的测试矩阵同此口径）。

理由：Linux 产物动态链接 glibc，构建机版本决定产物的 glibc 下限；`-latest` 会随 GitHub 迁移基础镜像而在无意中抬高该下限，破坏旧发行版的安装能力。Windows 侧 `-latest` 的镜像漂移会改变 MSVC 与打包工具链版本；macOS 侧漂移会改变 Xcode / SDK 版本，进而影响 dmg 与二进制的最低系统版本。

Linux 基线 MUST 选择能满足 Tauri v2 依赖（`libwebkit2gtk-4.1-dev`）的最老可用镜像，以尽量降低 glibc 下限；当前为 `ubuntu-22.04`（glibc 2.35）。

更换基线版本 MUST 由显式改动 runner 标签完成，并 MUST 在 `packaging/README.md` 记录当前基线与 glibc 下限。

#### Scenario: runner 未使用 latest
- **WHEN** 检查发布工作流各 job 定义
- **THEN** Linux job 的 `runs-on` 为 `ubuntu-22.04`、Windows job 为 `windows-2022`、macOS 矩阵为 `macos-14` / `macos-15-intel`，不存在任何 `-latest` 标签

#### Scenario: 基线变更需显式声明
- **WHEN** 维护者把任一平台基线改为其他版本
- **THEN** 改动以显式 runner 标签提交，并同步 `packaging/README.md` 中的基线与 glibc 下限说明
