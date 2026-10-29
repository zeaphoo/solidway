# release-pipeline Specification (delta: dmg-presentation)

## MODIFIED Requirements

### Requirement: macOS 公证与钉票

macOS 发布物 MUST 经 Apple 公证（notarization）后才可上传：构建任务 MUST 将签名后的 `.app`（zip 形态）提交 `notarytool` 并等待公证通过；通过后 MUST 先对 `.app` 钉票（stapling），再由已钉票的 `.app` 组装 dmg（安装盘布局随组装进入），MUST 将组装后的 dmg 单独提交 `notarytool` 并等待通过，通过后 MUST 对 dmg 钉票，使 `.app` 与 dmg 均携带公证票据（离线首开不依赖 Apple 在线校验）。dmg 有独立校验和，MUST NOT 复用 `.app` 的公证结果直接钉票。

公证提交、钉票与 dmg 组装 MUST 由可独立执行的脚本提供（`scripts/notarize-mac.sh`），CI 与本机共用同一脚本；dmg 组装（含安装盘布局）MUST 由 `scripts/package-mac.sh` 与 `scripts/notarize-mac.sh` 共用同一实现（`scripts/dmg-layout.sh`），本机与 CI 发布的 dmg MUST 布局一致。

#### Scenario: 公证通过后双钉票
- **WHEN** `.app` 公证通过
- **THEN** `.app` 先钉票；dmg 由已钉票的 `.app` 组装后单独提交公证，通过后再钉票，二者均含公证票据

#### Scenario: dmg 公证失败
- **WHEN** 组装后的 dmg 提交公证被拒或等待超时
- **THEN** 任务失败，dmg 不钉票、不上传

#### Scenario: 脚本独立可执行
- **WHEN** 在本机 `scripts/package-mac.sh` 产出的已签名 `dist/mac` 产物上执行 `scripts/notarize-mac.sh`
- **THEN** 完成公证、双钉票与验证，产出的 dmg 与本机构建路径布局一致

#### Scenario: 本机构建链不受影响
- **WHEN** 本机执行 `scripts/package-mac.sh`（不做公证）
- **THEN** 无需公证凭据即可产出布局 dmg；签名身份按「本机构建身份解析」解析（无内置默认身份，不存在自签默认出口）

## ADDED Requirements

### Requirement: macOS dmg 安装盘呈现

macOS dmg MUST 以安装盘交付：卷内 `Solidway.app` 旁 MUST 提供 `/Applications` 符号链接；Finder 窗口 MUST 固定 660×420，`Solidway.app` 居左、`Applications` 链接居右，无关界面元素 MUST 隐藏。

窗口背景 MUST 使用 Finder 默认呈现：MUST NOT 依赖自定义背景图或背景色。macOS 26 起 Finder 忽略 `.DS_Store` 的 `backgroundImageAlias` 与 `backgroundColor`，图标下方水印底由系统自动生成；打包链 MUST NOT 写背景相关 `.DS_Store` 键。

`.DS_Store` MUST 在无头环境由 python3 + `ds_store` 生成并脚本化进打包链，MUST NOT 依赖 Finder / AppleScript 界面自动化；MUST 只写窗口 `bwsp`、图标 `Iloc` 与 `icvp.arrangeBy`。

#### Scenario: 打开 dmg 见安装引导
- **WHEN** 用户在 Finder 打开 dmg
- **THEN** 窗口为 660×420，Solidway.app 居左、Applications 链接居右，无无关界面元素；窗口为 Finder 默认呈现（无自定义背景）

#### Scenario: 拖入 Applications 安装
- **WHEN** 用户把 Solidway.app 拖到 Applications 链接
- **THEN** 安装完成，从 Applications 启动可用

#### Scenario: 无头生成 .DS_Store
- **WHEN** 在 CI 无头环境执行打包链
- **THEN** `.DS_Store` 由 python3 + `ds_store` 生成成功，不依赖 Finder / AppleScript，且不含背景相关键

#### Scenario: CI 与本机布局一致
- **WHEN** 对比本机构建 dmg 与 CI 发布 dmg
- **THEN** 二者窗口尺寸、图标位置与 `/Applications` 链接一致（共用同一组装实现；背景均为 Finder 系统默认）

### Requirement: 本机构建身份解析

`scripts/package-mac.sh` 的签名身份 MUST 按以下顺序解析：`MAC_SIGN_IDENTITY` 显式设置时 MUST 优先使用，且 MUST 校验该身份存在于钥匙串；未设置时 MUST 自动探测钥匙串中**唯一**的 `Developer ID Application` 身份并使用；探测到多个或零个候选时 MUST 失败，并给出设置 `MAC_SIGN_IDENTITY` 的明确指引。

脚本 MUST NOT 内置任何默认身份名。

#### Scenario: 唯一 Developer ID 身份自动使用
- **WHEN** 未设置 `MAC_SIGN_IDENTITY` 且钥匙串恰有一个有效的 `Developer ID Application` 身份
- **THEN** 使用该身份签名

#### Scenario: 显式设置优先
- **WHEN** `MAC_SIGN_IDENTITY` 已设置且该身份存在于钥匙串
- **THEN** 使用显式身份签名（不执行自动探测）

#### Scenario: 显式身份不存在
- **WHEN** `MAC_SIGN_IDENTITY` 指定的身份不在钥匙串
- **THEN** 失败并提示找不到该身份

#### Scenario: 多个候选身份
- **WHEN** 钥匙串有多个有效的 `Developer ID Application` 身份且未设置 `MAC_SIGN_IDENTITY`
- **THEN** 失败并指引显式设置 `MAC_SIGN_IDENTITY` 选择其一

#### Scenario: 无 Developer ID 身份
- **WHEN** 钥匙串没有任何有效的 `Developer ID Application` 身份且未设置 `MAC_SIGN_IDENTITY`
- **THEN** 失败并指引（导入 Developer ID 证书或显式设置 `MAC_SIGN_IDENTITY`），不产出任何包

### Requirement: 应用版权信息

macOS 应用 bundle 的版权信息 MUST 为 `Copyright © 2026 solidway.xyz`（`app/tauri.conf.json` 的 `copyright` 字段，经 Info.plist 体现）。

#### Scenario: 显示简介可见版权
- **WHEN** 在 Finder 对 Solidway.app 执行「显示简介」或读取其 Info.plist
- **THEN** 版权信息为 `Copyright © 2026 solidway.xyz`
