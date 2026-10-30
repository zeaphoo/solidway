# Packaging

## GitHub Release 产物（推荐分发方式）

发布只由 **`v*` 标签**触发（`.github/workflows/release.yml`），一次 tag 产出并挂载：

| 平台 | 产物 | 构建脚本 |
|---|---|---|
| Linux 服务 | `solidway_<ver>-1_<arch>.deb`、`solidway-<ver>-1.<arch>.rpm`（systemd） | `scripts/package-linux.sh` |
| Linux 客户端 | `solidway-app_<ver>_<arch>.deb`、`solidway-app_<ver>_<arch>.AppImage`（均自带网关） | `scripts/package-linux-app.sh` |
| Windows 客户端 | `solidway-app_<ver>_x64-setup.exe`（NSIS，中英双语）、`solidway-app_<ver>_x64_<locale>.msi`、`solidway-app_<ver>_x64_portable.zip` | `scripts/package-windows.sh` |
| macOS 客户端 | `solidway-app_<ver>_<arch>.dmg`（x86_64 / arm64 各一份，已公证） | `scripts/package-mac.sh` + `scripts/notarize-mac.sh` |

命名约定：**客户端产物统一 `solidway-app_<ver>_<arch>[.<ext>]`**，服务包统一 `solidway_<ver>-<rev>_<arch>`；
架构名按平台惯例（Linux `amd64`/`arm64`、Windows `x64`、macOS `x86_64`/`arm64`）。

**预发布版本写法**：清单里的 semver 必须带连字符（如 `0.5.0-rc.1`，Cargo/Tauri 拒绝 `0.5.0rc1`），
而 deb / rpm 的版本字段用 `~` 表达预发布（`solidway_0.5.0~rc.1-1_amd64.deb`、`solidway-0.5.0~rc.1-1.x86_64.rpm`），
排序仍低于同号正式版：`cargo-deb` 自动转换，rpm 由 `scripts/package-linux.sh` 显式映射（RPM 的 `Version` 不允许 `-`）。
**Windows MSI** 的 `ProductVersion` 同样只接受数字（`major.minor.patch[.build]`），`scripts/package-windows.sh`
把预发布里的第一个数字映射到 build 段（`0.5.0-rc.1` → `0.5.0.1`），文件名与 NSIS / dmg / deb / rpm 仍是 `0.5.0-rc.1`。

便携版只以 zip 形式发布：zip 内的 `SolidwayApp.exe` / `solidway.exe` 不单独作为 Release 资产上传。

流程：

```bash
# 1) 清单版本一致（脚本会校验，否则 tag 构建直接失败）
./scripts/check-release-version.sh          # 应输出当前版本，如 0.5.0-rc.1
# 2) 写 CHANGELOG：把 [Unreleased] 的内容落到 `## [<version>] - <date>` 段
#    （缺该段时 verify 阶段直接失败，不会进入构建）
./scripts/release-notes.sh 0.5.0-rc.1      # 预览将作为 Release 正文的内容
# 3) 打 tag 并推送（tag 必须等于 v<version>）
git tag v0.5.0-rc.1 && git push origin v0.5.0-rc.1
```

Release 正文来自 `CHANGELOG.md` 的对应版本段（`scripts/release-notes.sh` 抽取），不再使用 GitHub
自动生成的提交汇总。

版本一致性由 `scripts/check-release-version.sh` 在构建前强制校验（`app/tauri.conf.json`
与全部 crates / app 的 `Cargo.toml`，且 tag 必须等于 `v<version>`）。

macOS 发布产物需要 **Developer ID 签名 + Apple 公证**，仓库 secrets 共 6 项（缺失即失败，不产出未签名/未公证包）：

| secret | 说明 |
|---|---|
| `MAC_CERT_P12_BASE64` | `Developer ID Application` 证书及其完整链的 p12（base64） |
| `MAC_CERT_PASSWORD` | p12 口令 |
| `MAC_SIGN_IDENTITY`（必填） | 签名身份全名，如 `Developer ID Application: <姓名> (<TeamID>)` |
| `MAC_NOTARY_API_KEY_ID` | App Store Connect API key id |
| `MAC_NOTARY_API_ISSUER_ID` | App Store Connect issuer id |
| `MAC_NOTARY_API_KEY_P8_BASE64` | API key 的 `.p8` 私钥（base64） |

公证以 App Store Connect API key 认证（`notarytool --key … --key-id … --issuer-id …`），
`.p8` 只在运行时落到临时文件、用后即删；真实凭据永不入仓库。

只想演练而不发 Release：手动触发 `Release` 工作流，`publish=false`（可选 `with_mac=false`
跳过 macOS，若尚未配置 secrets）。

`app/tauri.conf.json` 的 `identifier` 为 `xyz.solidway.desktop`（域名 `solidway.xyz` 的反向 DNS，命名约定 `xyz.<项目>.<端>`）。发布前没有历史用户，因此**不做数据目录迁移**：老开发者本机由旧 identifier 派生的目录（如 `~/Library/Application Support/xyz.solidway`、`%APPDATA%\xyz.solidway`）需要手工改名或删除。

**Linux 客户端依赖服务包**：客户端不内嵌 `solidway`，按 PATH 使用 `deb`/`rpm` 安装的
`/usr/bin/solidway`。因此安装顺序是「先服务包、后客户端」（见下文「与 Tauri 桌面应用的关系」）。

## Linux service packaging

产出 systemd 服务安装包：`.deb` 与 `.rpm`。安装后包含：

- `/usr/bin/solidway`
- `/usr/lib/systemd/system/solidway.service`
- 系统用户 / 组 `solidway`
- 数据目录 `/var/lib/solidway`

## 构建

安装 Rust 打包工具：

```bash
cargo install cargo-deb --locked
cargo install cargo-generate-rpm --locked
```

然后：

```bash
./scripts/package-linux.sh
```

默认输出到 `dist/linux/`：

```text
solidway_<version>-1_<arch>.deb
solidway-<version>-1.<arch>.rpm
```

脚本会先构建 `web/dist`，再执行 `cargo build --release -p solidway`。
二进制动态链接 glibc，建议在**目标发行版或目标最低版本**的构建机上打包，
避免 GLIBC 版本不兼容。

如果前端已构建，可跳过：

```bash
SKIP_WEB_BUILD=1 ./scripts/package-linux.sh
```

## 安装

```bash
sudo apt install ./solidway_0.1.0-1_amd64.deb
# 或
sudo rpm -ivh solidway-0.1.0-1.x86_64.rpm
```

安装脚本会创建 `solidway` 用户、初始化 `/var/lib/solidway`、启用并启动服务。

首次启动的 admin 引导令牌在 journal 中：

```bash
sudo journalctl -u solidway -n 100 | grep 'solidway: bootstrap'
```

## 运维

```bash
systemctl status solidway
systemctl restart solidway
systemctl stop solidway
journalctl -u solidway -f
```

数据目录：

```text
/var/lib/solidway/
├── solidway.db
├── usage_detail.db
├── master.key
└── tls/
```

卸载包不会删除 `/var/lib/solidway`（包含数据库、master key 和 TLS 私钥）。
彻底删除前请先备份，然后手工：

```bash
sudo rm -rf /var/lib/solidway
sudo userdel solidway
sudo groupdel solidway
```

内置 HTTPS 需要 80/443 时，unit 已授予 `CAP_NET_BIND_SERVICE`；如果前面有反向代理，
可以删除 unit 中的 `AmbientCapabilities` / `CapabilityBoundingSet` 两行。

## 与 Tauri 桌面应用的关系

本目录只打 **headless service**。桌面应用 `app/` 是独立的 Tauri 2 项目，
其安装包由 Tauri bundler 生成（`deb` / `rpm` / `AppImage` 等）。

桌面应用运行时会按以下顺序寻找 `solidway`：

1. 应用可执行文件同目录
2. 开发期仓库 `target/<profile>/solidway`
3. `PATH`

桌面客户端包已**自带** `solidway`（打包进 `/usr/lib/Solidway/solidway`），因此单装客户端即可运行：
壳的查找顺序是「可执行文件同目录 → 应用资源目录 → 开发期 target → PATH」。service 包因此是**可选**的
——需要 systemd 常驻（服务器形态）时才装；两者同时安装时，客户端仍优先使用自带的副本，互不干扰。

桌面应用自身用 Tauri CLI 打包，脚本封装了构建、网关注入与产物收集：

```bash
./scripts/package-linux-app.sh          # 产出 dist/linux-app/ 下的 deb + AppImage
# 前端已构建时可跳过：SKIP_WEB_BUILD=1 ./scripts/package-linux-app.sh
```

产物：

```text
solidway-app_<version>_<arch>.deb      # 包名 solidway-app（避免与服务包 solidway 同名）
solidway-app_<version>_<arch>.AppImage  # 单文件，自带 webkit 与网关
```

两者都把网关二进制打进 `usr/lib/Solidway/solidway`（bundle.resources），只装客户端即可运行；
`deb` 的 `Depends` 已声明 `libwebkit2gtk-4.1-0` / `libgtk-3-0` / `libayatana-appindicator3-1`，
AppImage 则自带 webkit，不依赖系统版本。

Linux 构建需要 Tauri 的系统依赖（`libwebkit2gtk-4.1-dev`、`libayatana-appindicator3-dev`、
`librsvg2-dev`、`libgtk-3-dev`、`patchelf` 等），以 Tauri 2 的 Linux 前置要求为准。


## Windows desktop app

产出 NSIS 安装包、MSI（可选）与便携 zip；`solidway.exe` 与 `SolidwayApp.exe` **必须同目录**
（桌面壳按「应用可执行文件同目录」查找网关），两类产物都已处理：

| 形态 | 做法 |
|---|---|
| 安装包（NSIS / MSI） | `app/tauri.windows.conf.json` 的 `bundle.resources` 把 `solidway.exe` 装到安装目录 |
| 便携 zip | 直接放 `SolidwayApp.exe` + `solidway.exe` + `README.txt`，免安装 |

```bash
./scripts/package-windows.sh
# 前端已构建时可跳过：SKIP_WEB_BUILD=1 ./scripts/package-windows.sh
# 没有 WiX 下载条件时只出 NSIS：TAURI_BUNDLES=nsis ./scripts/package-windows.sh
```

前置：Windows + Git Bash；`cargo` / `pnpm` / `node` / `cargo tauri`（`cargo install tauri-cli --locked`）。
脚本会先构建 `web/dist`、`cargo build --release -p solidway`，把 `solidway.exe` 临时放进
`app/`（`.gitignore` 已忽略），再让 Tauri bundler 打安装包。

默认输出到 `dist/windows/`：

```text
solidway-app_<version>_x64-setup.exe      # NSIS（中英双语 + 语言选择）
solidway-app_<version>_x64_en-US.msi      # WiX，按语言各一份（TAURI_BUNDLES 含 msi 时）
solidway-app_<version>_x64_zh-CN.msi
solidway-app_<version>_x64_portable.zip # 便携版（两个 exe 同目录）
```

### 安装器语言与安装路径

- **NSIS（默认）**：`bundle.windows.nsis` 配置 `["SimpChinese", "English"]` +
  `displayLanguageSelector: true`——系统语言命中就用对应语言，未命中时弹语言选择。
  **改了语言配置必须重新打包**：安装器本身内嵌语言，旧产物不会自动带上中文。
- **MSI（可选）**：`bundle.windows.wix.language = ["en-US", "zh-CN"]`，每个语言一个
  MSI，文件名保留 locale 后缀（`…_en-US.msi` / `…_zh-CN.msi`）。
- **安装路径统一为 `C:\Program Files\Solidway`**：
  - NSIS 设了 `installMode: "perMachine"`（默认是 `currentUser`，会装到
    `%LOCALAPPDATA%\Solidway`，与 MSI 不一致）；
  - MSI（WiX）没有安装模式配置，一直是 per-machine；
  - 两者都需要管理员权限（UAC）。
- **WebView2**：`webviewInstallMode` 显式设为 `downloadBootstrapper`（静默安装），
  缺运行时的机器装完即可用。

### 命名：为什么是 `SolidwayApp.exe`

Windows 与 macOS 默认的文件系统**不区分大小写**：`Solidway.exe` 与 `solidway.exe`
会被当成同一个文件，打包时两者互相覆盖，最终只剩一个。因此应用主程序名定为
`SolidwayApp.exe`（`tauri.conf.json` 的 `mainBinaryName`），与网关 `solidway.exe`
在**忽略大小写后仍然不同**。脚本会在打包后断言两个文件都存在，安装包内容也会用
7-Zip 列表校验。

### 未签名与运行前提

- **未做代码签名**：首次运行安装包/便携版时 SmartScreen 会提示「未知发布者」，
  选择「更多信息 → 仍要运行」；企业环境可用 GPO/信任策略放行。
- 需要 **WebView2 运行时**：Windows 10 1803+ 通常随 Edge 已安装；缺失时安装包会
  引导安装（Tauri 的 bootstrapper），便携版需用户自行安装。
- 数据目录为 `%APPDATA%\xyz.solidway.desktop`；Windows 上该目录不得位于共享目录（主密钥依赖目录 ACL）。

## macOS desktop app

产出 **Developer ID 签名 + Apple 公证**的 `Solidway.app` 与 `solidway-app_<version>_<arch>.dmg`
（hardened runtime，禁 ad-hoc；`.app` 与 dmg 双钉票，离线首开不依赖在线校验）。dmg 为**安装盘**：
`Solidway.app` 居左、`Applications` 链接居右，窗口固定 660×420；窗口背景使用 Finder 默认呈现
（macOS 26 起系统忽略 dmg 自定义背景图/纯色，打包链不写背景键）。
mac 侧是**纯桌面应用**：随开随用、退出即停，`app/` 启动并托管 `solidway` 子进程；没有 launchd 服务包。

### 构建前置检查项（逐项确认）

- [ ] `cargo` / `pnpm` / `node` / `python3` / `codesign` / `hdiutil` / `security` / `xcrun` 可用；
- [ ] `cargo tauri --version` 可用；缺失时执行 `cargo install tauri-cli --locked`；
- [ ] Keychain 中有 `Developer ID Application` 代码签名身份（`security find-identity -v -p codesigning`）。
      未设置 `MAC_SIGN_IDENTITY` 时脚本只接受**唯一**的 `Developer ID Application` 身份；多个/没有候选都会失败
      并提示显式设置（**不提供跳过签名开关、不回退 ad-hoc/默认自签**）；
- [ ] 公证凭据已就绪：`MAC_NOTARY_API_KEY_ID` / `MAC_NOTARY_API_ISSUER_ID` /
      `MAC_NOTARY_API_KEY_P8_BASE64`（或 `MAC_NOTARY_API_KEY_P8` 指向已解码的 `.p8` 文件）；
- [ ] `app/tauri.conf.json` 的 `version` 与 `crates/solidway`、`crates/solidway_api`、
      `crates/solidway_core`、`app` 的版本一致（脚本会校验，不一致即失败）；
- [ ] `web/dist/index.html` 存在（不设置 `SKIP_WEB_BUILD=1` 时脚本会先构建）。

```bash
# 默认自动探测唯一的 Developer ID Application 身份
./scripts/package-mac.sh
# 钥匙串有多个候选时显式指定（名字需与 security find-identity 输出完全一致）：
MAC_SIGN_IDENTITY="Developer ID Application: <姓名> (<TeamID>)" ./scripts/package-mac.sh
# 前端已构建时可跳过：SKIP_WEB_BUILD=1 ./scripts/package-mac.sh
# 公证 + 双钉票 + Gatekeeper 验收（发布必需；本机也可单独执行）：
./scripts/notarize-mac.sh
```

默认输出到 `dist/mac/`：

```text
Solidway.app
solidway-app_<version>_<arch>.dmg   # 安装盘（窗口/图标布局 + Applications 链接）
```

打包顺序固定（顺序错会产生不含 `solidway` 或未签名的坏 dmg）：
`cargo build --release -p solidway` → `cargo tauri build --bundles app` →
post-copy `solidway` 到 `Contents/MacOS/` → 非主程序内层 Mach-O 先签、`.app` 外层后签（`SolidwayApp` 随 bundle 签） →
`scripts/dmg-layout.sh` 组装 dmg（staging：`.app` + `/Applications` 链接；在 staging 内生成
`.DS_Store`，再 `hdiutil` 建 HFS+ UDZO）→ 挂载 dmg 断言 `solidway`、链接、`.DS_Store` 与
`codesign --verify --deep --strict`。**不得**使用 `cargo tauri build --bundles dmg`
（bundler 会重建 `.app`，覆盖 post-copy 与签名结果）。

`scripts/notarize-mac.sh` 必须在该链之后执行：把已签名 `.app` 打成 zip 提交公证，通过后先钉票
`.app`；再由已钉票的 `.app` 经**同一** `scripts/dmg-layout.sh` 重建布局 dmg，
**dmg 有独立校验和，必须单独提交公证**，通过后再钉票（覆盖 `dist/mac/` 中未公证的 dmg）。

Finder 布局与 `.DS_Store` 由 `scripts/python/gen_dmg_ds_store.py`（vendored `ds_store`/`mac_alias`）生成：
只写窗口 `bwsp`、图标 `Iloc` 与 `icvp.arrangeBy`，不写背景相关键；不依赖 Finder/AppleScript。
macOS 26 起 Finder 忽略 dmg 自定义背景（图片与纯色均不渲染），因此交付干净白窗，不再维护背景素材。

### 安装

```bash
ditto dist/mac/Solidway.app /Applications/Solidway.app
open /Applications/Solidway.app
```

签名与公证自检（构建机）：

```bash
codesign --verify --deep --strict dist/mac/Solidway.app
codesign -dv --verbose=2 dist/mac/Solidway.app   # Authority=Developer ID Application、flags 含 runtime
xcrun stapler validate dist/mac/Solidway.app
xcrun stapler validate dist/mac/solidway-app_<version>_<arch>.dmg
# dmg 本体不做 spctl 评估（无 Mach-O 签名可评）；挂载后验收内部 .app：
hdiutil attach -nobrowse -readonly dist/mac/solidway-app_<version>_<arch>.dmg
spctl -a -vv /Volumes/Solidway/Solidway.app
hdiutil detach /Volumes/Solidway
# 期望 spctl 输出分行含 accepted 与 source=Notarized Developer ID
```

### Gatekeeper / quarantine 说明

发布产物经 Apple 公证且 `.app` 与 dmg 双钉票：经浏览器下载 / AirDrop 带 quarantine 时
Gatekeeper 仍可离线验证票据，双击直接打开，不需要手动放行。

`AKSL Software` 本机自签证书已退役；`scripts/package-mac.sh` **没有默认自签身份**，默认只产出
Developer ID 签名包。若确需临时自签（仅本机冒烟/应急，**不得对外分发**）：自行重建自签身份并导入钥匙串，
再显式设置 `MAC_SIGN_IDENTITY=<自签身份名>` 后运行；不做公证的包 `spctl` 预期不通过。

### 冒烟与数据目录

`data_root` 固定为 `~/Library/Application Support/xyz.solidway.desktop`（不读配置文件、
不支持应用专属环境变量覆盖）。冒烟会写真实数据目录，按下面程序隔离：

```bash
DATA="$HOME/Library/Application Support/xyz.solidway.desktop"
STAMP="$(date +%Y%m%d-%H%M%S)"
[ -d "$DATA" ] && mv "$DATA" "$DATA.bak-$STAMP"   # 备份真实数据
# 启动 app → 状态转「运行中」→ 控制台可达（端口以 solidway: listening addr= 为准，缺省 6565）
# → 退出 app → 确认 solidway 子进程已停止
```

冒烟后的处理：

- **验证通过**：删除本次冒烟产生的 `"$DATA"`，再恢复备份：
  `[ -d "$DATA.bak-$STAMP" ] && rm -rf "$DATA" && mv "$DATA.bak-$STAMP" "$DATA"`；
- **验证失败**：保留 `"$DATA"` 与 `bak-$STAMP` 供排查，不要删除；
- **备份堆积**：`bak-<时间戳>` 只保留**最近 2 份**，更早的在确认最新备份可恢复后手工删除：

```bash
ls -dt "$HOME/Library/Application Support"/xyz.solidway.desktop.bak-* 2>/dev/null | tail -n +3
# 确认上面列出的都是可删旧备份后：| xargs rm -rf
```

### CI 与分发边界

- **CI 只在 `v*` 标签上构建 mac 产物**（ADR 0018）：签名身份经 secrets 导入临时 keychain，
  构建后 `codesign --verify --deep --strict` 自检，随后 `scripts/notarize-mac.sh` 完成公证、
  双钉票与 `spctl` 验收；只有已公证并通过验收的 `.dmg` 才上传。分支 push 不构建 mac 包。
- secrets 缺失时 job 直接失败，**不提供跳过签名/公证开关、不回退 ad-hoc 或自签**（防止未签名/未公证产物流出）。
- **公证失败即发布失败**：提交被拒、等待超时、钉票或 `spctl` 验收不通过都使 job 失败，不上传、不降级。
- 本机 `./scripts/package-mac.sh` 与 CI 走同一签名构建链；`scripts/notarize-mac.sh` 同样可在本机独立执行，
  用于复现或应急公证。
- mac 矩阵：arm64 用 `macos-14`，Intel（x86_64）用 `macos-15-intel`（GitHub 已下线 `macos-13`）；
  若该镜像未来下线，换当时的 Intel 镜像并同步更新 workflow。
- Windows 产物在 `windows-latest` 上构建（同为 tag 触发），不做签名、不依赖 secrets；安装包内
  `solidway.exe` 的落点由 `bundle.resources` 保证，首次发布建议实机确认安装目录。

### 与 Linux service 包的关系

- mac 只分发桌面 app（无 launchd / 无系统服务），与 `dist/linux/` 的 deb/rpm 互不影响；
- `solidway` 二进制**随 `.app` 分发**（`Contents/MacOS/solidway`），不依赖 `PATH` 或已安装
  Linux service 包；桌面壳定位顺序仍为「应用可执行文件同目录 → 开发期工作区产物 → `PATH`」。
