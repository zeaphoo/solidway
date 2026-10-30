# Changelog

本项目的所有重要变更记录在此文件。

格式遵循 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循
[语义化版本](https://semver.org/lang/zh-CN/)。每个版本的条目由 `scripts/release-notes.sh`
抽取为对应 GitHub Release 的正文（发布闸门会校验该版本条目存在）。

## [Unreleased]

## [0.5.0-rc.2] - 2026-10-30

### Changed

- **产物命名带操作系统标记**：`solidway[-app]_<ver>_<os>_<arch>[.<ext>]`（`<os>` = `linux` / `windows` / `macos`），服务包为 `solidway_<ver>-<rev>_<os>_<arch>`，例如 `solidway_0.5.0~rc.2-1_linux_amd64.deb`、`solidway-app_0.5.0-rc.2_macos_arm64.dmg`、`solidway-app_0.5.0-rc.2_windows_x64_portable.zip`，便于在 Release 资产列表里辨认平台。
- **固定构建机**：发布与 CI 的 Ubuntu job 改为 `ubuntu-22.04`（glibc 2.35，比 24.04 的 2.39 兼容更老的发行版）、Windows job 改为 `windows-2022`、macOS arm64（发布 + CI）改为 `macos-15`（`macos-14` 镜像已 deprecated）、Intel leg 保持 `macos-15-intel`；三平台均不再使用 `-latest`，避免基础镜像迁移被动抬高依赖下限或改变工具链。

### Fixed

- **Linux 22.04 构建失败**：`rust-cache` 默认只按 `runner.os` 分桶，把 24.04（glibc 2.39）上编译的 `aws-lc-sys` 目标文件复用到 22.04（glibc 2.35）上，链接时报 `undefined symbol: __isoc23_sscanf` / `__isoc23_strtol`；现所有缓存步骤的 `key` 带上 runner 镜像标签（`ubuntu-22.04` / `macos-15` / `macos-15-intel` / `windows-2022`）。
- **发布资产**：Windows 便携版的 `solidway.exe` / `SolidwayApp.exe` 不再作为独立 Release 资产上传（发布物只有便携 zip，避免重复下载与体积误解）；发布收集步骤跳过 `*/portable/*`。

## [0.5.0-rc.1] - 2026-10-30

首个公开预发布版本。

### Added

- **数据面 / 控制面单进程**：一个静态二进制内含转发引擎、管理 API 与内嵌控制台，运行期不联系任何外部控制面。
- **保真转发**：三种入站协议族（OpenAI Chat Completions、OpenAI Responses、Anthropic Messages）各按自身语义转发；请求体、响应体与流式帧按原始分块边界透传，唯一改写点是别名生效时的顶层 `model`，不做跨协议翻译、不改写提示词。
- **池化凭据与选号**：路由即凭据池；支持 `round_robin` / `latency` / `uptime` / `auto` 策略与档位优先、会话粘性；首字节前故障静默换号。
- **健康与失败分类**：`401` 退役、限流 / 额度 / 权限类进入定时冷却、请求本身错误不摘除；探测自动恢复；事件（incidents）与 webhook 告警带重试与按凭据节流。
- **控制台（中 / 英）**：路由、Provider 凭据（加密存储 + 掩码展示 + 模型发现）、API Key、按路由与全局用量、事件、用户、系统设置、内置 TLS（ACME `http-01` / `tls-alpn-01`）。
- **用量归属**：小时 / 天聚合桶（请求、错误、换号、TTFB、延迟直方图、token、字节），可按路由、API Key、Provider、模型、owner 维度归因；逐条明细可选保留并按比例采样。
- **Provider 目录**：基于 models.dev 生成的预设，含 Vertex / Azure / Bedrock（SigV4）端点模板。
- **桌面客户端**：Linux（deb / AppImage）、macOS（dmg，Developer ID 签名 + 公证）、Windows（NSIS / MSI + 便携 zip），内置网关与托盘。
- **发布流水线**：`v*` 标签触发，校验版本与 CHANGELOG 条目后构建并挂载全部产物。

### Fixed

- **token 统计恒为 0**：上游按 `accept-encoding` 压缩响应时，网关此前把压缩字节直接当 JSON / SSE 解析，导致 usage 永远缺失。现按 `content-encoding`（`gzip` / `deflate` / `br` / `zstd`）**旁路解码后再解析**，转发字节与响应头保持原样。
- **流式用量缺失**：OpenAI Chat Completions 流式请求未声明 `stream_options` 时，由数据面补 `include_usage`（已声明取值、非流式与其他协议族不注入），否则上游不回传 usage。
- **Provider 凭据掩码**：由「仅末 4 位」改为「首尾各 4 位」，并在读取时按当前规则从密文重算，存量记录无需重新保存；编辑弹窗以 placeholder 直接显示当前掩码。
- **模型清单**：默认只显示一个模型 id，需要对外改名时才展开「上游 id / 对外名称」两列；重新获取模型按上游 id 合并去重并恢复上游列表顺序，改名行不再重复插入。
- **打包**：`v*` 发布时 RPM 构建因 semver 预发布 `-`（RPM `Version` 不允许）失败，现将 `-` 映射为 `~`（`0.5.0-rc.1` → `0.5.0~rc.1`），deb / rpm 排序均低于同号正式版。
- **Windows MSI**：WiX 的 `ProductVersion` 只接受数字，非数字预发布标识符（`rc.1`）会让 msi 打包失败；现由 `scripts/package-windows.sh` 把预发布里的数字映射到 build 段（`0.5.0-rc.1` → `0.5.0.1`），产物文件名仍为 `0.5.0-rc.1`。
- **macOS 分发**：dmg 安装盘布局（Applications 链接与图标定位）、Developer ID 签名 + Apple 公证双钉票、Gatekeeper 验收。

[Unreleased]: https://github.com/zeaphoo/solidway/compare/v0.5.0-rc.2...HEAD
[0.5.0-rc.2]: https://github.com/zeaphoo/solidway/releases/tag/v0.5.0-rc.2
[0.5.0-rc.1]: https://github.com/zeaphoo/solidway/releases/tag/v0.5.0-rc.1
