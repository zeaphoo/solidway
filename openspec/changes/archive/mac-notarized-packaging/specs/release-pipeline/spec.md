# release-pipeline Specification (delta: mac-notarized-packaging)

## MODIFIED Requirements

### Requirement: 发布产物集

发布工作流 SHALL 产出并挂载以下产物：

- Linux 服务包：`.deb` 与 `.rpm`（含 systemd unit），由 `scripts/package-linux.sh` 构建；
- Linux 桌面客户端：`.deb` 与 `.AppImage`，由 Tauri bundler 构建，MUST 自带 `solidway` 网关二进制（`bundle.resources`，落点为打包器资源目录）；`.deb` 的包名 MUST 与服务包 `solidway` 不同（使用 `solidway-app`），避免 dpkg 视为同名包互相替换；
- Windows 桌面客户端：NSIS（或 MSI）安装包 MUST 把 `solidway.exe` 安装到与应用可执行文件同一目录；另 MUST 提供便携 zip（`SolidwayApp.exe` + `solidway.exe` 同目录）。NSIS 安装器 MUST 提供简体中文与英文，并 MUST 在系统语言未命中时提供语言选择；MSI 产物 MUST 按语言分别输出（文件名带 locale 后缀）。

两种安装器 MUST 安装到同一路径（`C:\Program Files\<产品名>`）：NSIS MUST 使用 per-machine 安装模式（其默认 currentUser 会装到 `%LOCALAPPDATA%`，与 MSI 不一致）。应用主程序名 MUST 在忽略大小写后与 `solidway` 不同（Windows/macOS 文件系统不区分大小写），打包脚本 MUST 断言两个可执行文件都存在。Windows 产物 MAY 未做代码签名，但 MUST 在分发说明中标注 SmartScreen 影响与 WebView2 前提；
- macOS 桌面客户端：`.dmg`，由 `scripts/package-mac.sh` 构建、`scripts/notarize-mac.sh` 公证，签名口径 MUST 为 Developer ID Application 签名 + hardened runtime，发布物 MUST 为已公证产物（`.app` 与 dmg 均钉票）。

客户端产物文件名 MUST 统一以 `solidway-app_` 为前缀（含版本与架构），服务包 MUST 使用 `solidway_` 前缀，二者 MUST NOT 混淆；架构名 MUST 遵循各平台惯例（Linux `amd64`/`arm64`、Windows `x64`、macOS `x86_64`/`arm64`）。

任一产物缺失时发布任务 MUST 失败（MUST NOT 发布不完整产物集）。

#### Scenario: 产物齐全
- **WHEN** 所有构建任务成功
- **THEN** Release 页面包含 Linux 服务包、Linux 客户端包、Windows 安装包与便携 zip、macOS dmg

#### Scenario: 客户端与服务包文件名可区分
- **WHEN** 下载 Releases 页面的全部产物
- **THEN** 客户端产物均为 `solidway-app_<ver>_<arch>[.<ext>]`，服务包均为 `solidway_<ver>-<rev>_<arch>`，仅看文件名即可区分用途

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

### Requirement: macOS 构建的签名前提

macOS 构建任务 MUST 从仓库 secrets（`MAC_CERT_P12_BASE64`、`MAC_CERT_PASSWORD`、`MAC_SIGN_IDENTITY`）导入签名身份到临时 keychain，并在构建后执行 `codesign --verify --deep --strict` 自检。签名身份 MUST 为 `Developer ID Application` 证书对应身份；`MAC_SIGN_IDENTITY` 为必填 secrets，MUST NOT 内置默认身份名（个人账号身份名含个人姓名，不可硬编码进 workflow）。

secrets 缺失或签名自检失败时任务 MUST 失败，MUST NOT 产出未签名或 ad-hoc 签名的产物，MUST NOT 提供跳过签名的开关，MUST NOT 回退临时自签；失败信息 MUST 列出所需 secrets 名称清单与配置指引。

macOS 产物 MUST 按 runner 原生架构分别构建（x86_64 与 arm64 各一份）。

#### Scenario: 签名成功
- **WHEN** secrets 已配置且证书链完整
- **THEN** 产出通过 `codesign --verify --deep --strict` 的 dmg，并上传

#### Scenario: 未配置签名 secrets
- **WHEN** `MAC_CERT_P12_BASE64`、`MAC_CERT_PASSWORD` 或 `MAC_SIGN_IDENTITY` 为空
- **THEN** 任务立即失败并提示所需 secrets 与配置指引，不产出任何 mac 产物

#### Scenario: 签名身份不匹配
- **WHEN** 导入的证书不含 `MAC_SIGN_IDENTITY` 指定的 Developer ID Application 身份
- **THEN** `find-identity` / `codesign` 校验失败，任务失败

#### Scenario: 双架构产物
- **WHEN** macOS 矩阵两个 runner 均成功
- **THEN** Release 同时包含 arm64 与 x86_64 的 dmg

## ADDED Requirements

### Requirement: macOS 公证与钉票

macOS 发布物 MUST 经 Apple 公证（notarization）后才可上传：构建任务 MUST 将签名后的 `.app`（zip 形态）提交 `notarytool` 并等待公证通过；通过后 MUST 先对 `.app` 钉票（stapling），再由已钉票的 `.app` 重建 dmg 并对 dmg 钉票，使 `.app` 与 dmg 均携带公证票据（离线首开不依赖 Apple 在线校验）。

公证提交、钉票与 dmg 重建 MUST 由可独立执行的脚本提供（`scripts/notarize-mac.sh`），CI 与本机共用同一脚本；`scripts/package-mac.sh` 的本机构建链 MUST 保持不变（自签应急路径不受影响）。

#### Scenario: 公证通过后双钉票
- **WHEN** `.app` 公证通过
- **THEN** `.app` 先钉票，dmg 由已钉票的 `.app` 重建后再钉票，二者均含公证票据

#### Scenario: 脚本独立可执行
- **WHEN** 在本机 `scripts/package-mac.sh` 产出的已签名 `dist/mac` 产物上执行 `scripts/notarize-mac.sh`
- **THEN** 完成公证、双钉票与验证，无需修改 `package-mac.sh`

#### Scenario: 本机构建链不受影响
- **WHEN** 本机执行 `scripts/package-mac.sh`（不做公证）
- **THEN** 行为与改动前一致，自签应急路径保持可用

### Requirement: 公证凭据的 Secrets 注入

公证凭据 MUST 只经仓库 secrets 注入：App Store Connect API key 形态为 `MAC_NOTARY_API_KEY_ID`、`MAC_NOTARY_API_ISSUER_ID`、`MAC_NOTARY_API_KEY_P8_BASE64`。workflow 与仓库 MUST NOT 含任何明文凭据，MUST 只读取 secrets。

任一公证凭据缺失时任务 MUST 失败并输出配置指引（含 secrets 名称清单），MUST NOT 回退自签、MUST NOT 跳过公证。

#### Scenario: 凭据只经 secrets 注入
- **WHEN** workflow 运行公证步骤
- **THEN** 公证凭据只来自仓库 secrets，workflow 文本与仓库内不存在明文凭据

#### Scenario: 公证凭据缺失
- **WHEN** 任一 `MAC_NOTARY_*` secrets 为空
- **THEN** 任务失败并输出配置指引，不产出未公证产物

### Requirement: 公证失败即发布失败

公证提交被拒、等待超时、钉票失败或验证不通过时，任务 MUST 失败；MUST NOT 上传或发布未公证的 dmg，MUST NOT 降级为「仅签名」发布，MUST NOT 提供跳过公证的开关。

#### Scenario: 公证被拒
- **WHEN** `notarytool` 返回公证不通过（Invalid）
- **THEN** 任务失败并输出公证失败日志，dmg 不上传

#### Scenario: 公证等待超时
- **WHEN** 公证状态等待超过预算时间仍未通过
- **THEN** 任务失败，不产出、不发布任何 mac 产物

### Requirement: Gatekeeper 验收闸门

mac 产物上传前 MUST 以 `spctl -a -vv` 验证最终 dmg，输出 MUST 含 `accepted source=Notarized Developer ID`；验证不通过任务 MUST 失败。演练（`workflow_dispatch`，`publish=false`）产出的 dmg MUST 满足同一验收标准。

#### Scenario: spctl 验收通过
- **WHEN** 对最终 dmg 执行 `spctl -a -vv`
- **THEN** 输出含 `accepted source=Notarized Developer ID`，产物继续上传

#### Scenario: spctl 验收拒绝
- **WHEN** `spctl` 输出不含 `accepted source=Notarized Developer ID`
- **THEN** 任务失败，dmg 不上传

#### Scenario: 演练产物同标准
- **WHEN** 维护者以 `workflow_dispatch`（`publish=false`）演练
- **THEN** 产出的 dmg 已公证并通过 `spctl` 验收
