# Design — release-pipeline-v-tags

## 1. 触发：只认 `v*` tag

```yaml
on:
  push:
    tags: ['v*']
  workflow_dispatch:
    inputs:
      publish: { type: boolean, default: false }
      with_mac: { type: boolean, default: true }
```

| 方案 | 问题 | 结论 |
|---|---|---|
| **tag 触发（采用）** | 无（tag 即发布意图，天然节流） | ✅ |
| 每次 push 构建全部产物 | macOS runner 约 10x 计费；每次 push 都烧 | 否（现状 `ci.yml` 已覆盖构建正确性） |
| 手动 `workflow_dispatch` 为主 | 发布靠人点，产物与 tag 无强绑定 | 仅作演练入口 |

`workflow_dispatch` 只用于「第一次把流水线跑通」：默认 `publish=false`（只出 artifacts），`with_mac` 可关掉签名 job 以省额度。

## 2. 版本闸门：tag 必须等于清单版本

`scripts/check-release-version.sh [tag]`：

- 从 `app/tauri.conf.json` 取版本（Tauri 打包会用它命名 dmg / deb）；
- 与 `crates/{solidway,solidway_api,solidway_core,solidway_egress,solidway_transcode}`、`app/` 的 `Cargo.toml` 逐个比对；
- 给了 tag（`v*`）时再断言 `v<version> == tag`，非 `v` 前缀直接失败；
- 成功时把裸版本打到 stdout，供 job output 使用。

**为什么单独成脚本**：CI 与本机都能跑同一条闸门；比在 YAML 里内联 awk 更可测（本地已用 `v0.1.0` / `v0.2.0` 验过）。`scripts/package-mac.sh` 内部保留自己的校验（不变），两者职责重叠但互不依赖。

## 3. Job 拓扑

```txt
verify ──┬── linux-service ──┐
         ├── linux-app ──────┼── release（仅 tag 或 publish=true）
         └── mac (matrix 2) ─┘
```

- `verify`：ubuntu-latest，只做 tag/版本校验，输出 `version`。
- `linux-service`：ubuntu-latest；`cargo install cargo-deb cargo-generate-rpm`；`./scripts/package-linux.sh`；产物 `dist/linux/*.deb|*.rpm`。
- `linux-app`：ubuntu-latest；装 Tauri 2 的 Linux 依赖（WebKitGTK 4.1 / libayatana-appindicator / librsvg / patchelf 等）；`./scripts/package-linux-app.sh`；产物 `dist/linux-app/*.deb|*.AppImage`。
- `windows-app`：windows-latest；`cargo install tauri-cli`；`./scripts/package-windows.sh`；产物 `dist/windows/*.exe|*.msi|*.zip`。
- `mac`：`macos-13`（x86_64）+ `macos-14`（arm64）矩阵；导入 p12 → `./scripts/package-mac.sh` → 自检 → 上传 `dist/mac/*.dmg`。
- `release`：ubuntu-latest；下载全部 artifacts 到 `dist/`；`softprops/action-gh-release@v2` 创建/更新 Release（`generate_release_notes: true`，`fail_on_unmatched_files: true`）。

## 4. 三平台网关二进制的落点

原则统一为：**客户端包自带网关**，落点交给打包器，桌面壳用 Tauri 的 `resource_dir()` 查找
（顺序：可执行文件同目录 → 资源目录 → 开发期 target（仅 debug）→ PATH）。

| 平台 | 打包方式 | 落点 | 与查找顺序的关系 |
|---|---|---|---|
| Linux deb/rpm | `bundle.resources` | `/usr/lib/<产品名>/` | 由 `resource_dir()` 第 2 位命中 |
| Linux AppImage | `bundle.resources` | `$APPDIR/usr/lib/<产品名>/` | 同上 |
| Windows | `bundle.resources` | 安装目录 | 与可执行文件同目录，第 1 位即命中（去重） |
| macOS | 构建后 post-copy | `Contents/MacOS/` | 与可执行文件同目录，第 1 位即命中 |

早期版本让 Linux 客户端依赖服务包的 `/usr/bin/solidway`（省约 8 MB 安装包），实测结果是
"只装客户端必然打不开"，且 AppImage 失去「下载即用」的意义。改为自带后，服务包降级为
**可选**（需要 systemd 常驻时才装）；两者共存时客户端仍优先用同一个目录里的副本。

deb 包名：Tauri 的 deb 配置没有包名字段（`Package:` 由 productName 推导，会得到 `solidway`，
与服务包同名，dpkg 会当成同一个包升级并删掉服务包文件）。因此在 `package-linux-app.sh` 里
用 `dpkg-deb -R/-b` 重写 control 的 `Package: solidway-app` 并同步文件名。

## 5. Windows 客户端：资源注入 + 便携包

Windows 没有服务包形态，`solidway.exe` 必须与 `SolidwayApp.exe` 同目录（`app/src/config.rs`
的「可执行文件同目录」查找）。两种分发都保留：

| 形态 | 内容 | 做法 |
|---|---|---|
| NSIS / MSI 安装包 | `SolidwayApp.exe` + `solidway.exe` 安装到同一目录 | 打包期 `--config` 注入 `bundle.resources`（Windows 的资源目录就是安装目录） |
| 便携 zip | 两个 exe 同目录 + 说明 | Tauri 按 `mainBinaryName` 输出 `SolidwayApp.exe`，与 `solidway.exe` 一起压缩 |

为什么不用 `bundle.resources` 统一处理三平台：macOS 的资源目录是 `Contents/Resources`，
与「可执行文件同目录」不一致（因此 mac 继续 post-copy 进 `Contents/MacOS`）；Linux 有服务包
提供 `/usr/bin/solidway`（因此不内嵌）。三平台各自的摆放方式都是「让 `locate_solidway` 能找到」。

未做代码签名：无 Windows 代码签名证书，产物为未签名；SmartScreen 会提示「未知发布者」，
README 给出「仍要运行」说明。签名不在本次范围（需要 EV/OV 证书与 secrets 设计）。

`TAURI_BUNDLES` 可覆盖默认 `nsis,msi`：没有 WiX 下载条件的环境退化为 `nsis` 单安装包。

## 6. macOS 签名：secrets 与临时 keychain

| secret | 用途 |
|---|---|
| `MAC_CERT_P12_BASE64` | AKSL Software 及其 CA 完整链的 p12（base64） |
| `MAC_CERT_PASSWORD` | p12 口令 |
| `MAC_SIGN_IDENTITY`（可选） | 默认 `AKSL Software`，与脚本默认值一致 |

步骤：`security create-keychain` → `unlock` → `import -T /usr/bin/codesign` → `set-key-partition-list` → `find-identity` 断言 → 跑脚本 → `codesign --verify --deep --strict` 自检 → `security delete-keychain`（`always()`）。

**secrets 缺失即失败**：不留「跳过签名」分支，也不回退 ad-hoc —— 未签名/未公证的 mac 产物一旦流出，用户侧表现为「已损坏」，比构建失败更难排查（ADR 0011 D6）。错误信息里给出 secrets 名称与 ADR 链接。

备选（否）：

- **只构建不上传（artifact）**：mac 分发需要人下载后转发，等于把分发成本转给开发者，且 release 页缺 mac 会误导用户。
- **universal2 单包**：需要两套 `solidway` + app 二进制，脚本会大改；先按 runner 原生架构出两个 dmg。
- **不签名的 `.app`**：违反 ADR 0011（禁 ad-hoc）；且 Gatekeeper 直接拒绝。

## 7. 产物命名

| 来源 | 文件名 |
|---|---|
| `package-linux.sh` | `solidway_<ver>-1_<arch>.deb`、`solidway-<ver>-1.<arch>.rpm` |
| `package-linux-app.sh` | `solidway-app_<ver>_<arch>.deb`、`solidway-app_<ver>_<arch>.AppImage` |
| `package-windows.sh` | `solidway-app_<ver>_x64-setup.exe`（NSIS）、`solidway-app_<ver>_x64.msi`、`solidway-app_<ver>_x64_portable.zip` |
| `package-mac.sh` | `solidway-app_<ver>_<arch>.dmg` |

服务包与客户端包文件名前缀不同（`solidway_` vs `solidway-app_`），不会互相覆盖；两者用途在 Release notes 中说明。

## 8. 并发与失败策略

- `concurrency: release-<ref>`，`cancel-in-progress: false`：同一 tag 的重复推送不互相打断。
- `fail-fast: false`（mac 矩阵）：x86_64 失败不影响 arm64 上传，便于判断是证书还是架构问题。
- `release` 依赖全部 build job：任何一个失败就不发 Release（避免半套产物）。
- 不设 `continue-on-error`：任何软失败都会导致缺产物。

## 9. 风险

| 风险 | 缓解 |
|---|---|
| GitHub macOS runner 上 `kSecUseDataProtectionKeychain` 导致 `codesign` 找不到身份 | 导入后 `security find-identity` + `codesign --verify` 双重断言；失败信息直接指向证书链问题 |
| Tauri Linux 依赖在 ubuntu-latest(24.04) 上缺失 | 依赖列表按 Tauri 2 官方文档固定；AppImage 只做「构建」不运行，不需要 FUSE |
| 首次真实 tag 才发现流水线问题 | 先 `workflow_dispatch` 演练（`publish=false`），通过后再打 tag |
| 版本号漂移 | `verify` job 前置，失败即止 |
| 私钥上云（ADR 0011 原顾虑） | 已由负责人决策接受；secret 仅在本 job 的临时 keychain 存在，job 结束删除 |
| Windows 安装包里 `solidway.exe` 落点不对 | 用 `bundle.resources`（Windows 资源目录 = 安装目录）+ 便携 zip 兜底；首次 tag 后需实机确认安装目录内容 |
| Windows 无签名导致 SmartScreen 拦截 | README 说明；后续可加 `signtool` + 证书 secrets（另立 change） |
