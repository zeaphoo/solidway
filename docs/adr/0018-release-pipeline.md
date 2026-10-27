# ADR 0018 — 发布只由 `v*` 标签触发，CI 产出全部客户端与服务包

## 状态

已接受（2026-10-26）。恢复 ADR 0011 被撤销的「CI 打包」决策 4，但按 tag 触发与 secrets 前提重新界定。

## 背景

此前三类产物各有各的产出方式：Linux 服务包由 CI 每次 push 构建但只做 artifact；Linux 桌面客户端没有脚本；macOS 客户端只在本机打（ADR 0011 方案 B：私钥不上云 + mac runner 贵）。

结果：发版要凑齐三台机器与人肉步骤，tag 与清单版本之间没有闸门，产物没有统一的下载位置。ADR 0011 的「修订记录」已预留恢复路径：负责人决定后改为 tag 触发 + 签名 secrets。

## 决策

1. **唯一发布触发点是 `v*` 标签**。分支 push 不构建发布物（`ci.yml` 继续负责正确性验证）；`workflow_dispatch` 仅供演练，默认不创建 Release。
2. **版本一致性是硬闸门**：`scripts/check-release-version.sh` 校验 `app/tauri.conf.json` 与全部 crates/app 的版本，tag 必须等于 `v<version>`；失败即不构建。
3. **产物集固定**：Linux 服务包（deb/rpm）、Linux 桌面客户端（deb/AppImage）、Windows 客户端（NSIS/MSI 安装包 + 便携 zip）、macOS 客户端（dmg ×2 架构）。任一缺失则不发 Release。
4. **macOS 签名继续按 ADR 0011 的口径**（AKSL 自签 + hardened runtime、禁 ad-hoc、不公证）。CI 通过 secrets 在临时 keychain 导入身份，构建后自检；secrets 缺失即失败，不提供跳过开关。
5. **客户端包一律自带网关二进制，落点由打包器决定，壳用 `resource_dir()` 查找**：Linux deb/AppImage 走 `bundle.resources`（`/usr/lib/<产品名>/`、`$APPDIR/usr/lib/<产品名>/`）；Windows 走 `bundle.resources`（安装目录，另有便携 zip）；mac 保持 post-copy 进 `Contents/MacOS/`。服务包降级为可选（需要 systemd 常驻时才装）；Linux 客户端 deb 包名用 `solidway-app`，避免与服务包 `solidway` 同名。查找顺序见 `desktop-app` spec 的「启动 solidway」。
6. **Windows 暂不做代码签名**：无证书；安装包与便携包均为未签名，SmartScreen 会提示「未知发布者」。分发说明里写清放行方式与 WebView2 前提。
7. **版本号仍人工 bump**：流水线只做一致性校验，不做自动 bump / 自动打 tag。

## 被否方案

- **每次 push 构建全部产物**：macOS runner 约 10x 计费，且与发布意图无关。
- **继续只在本机打包**：三平台人肉步骤无法长期维持，tag 与产物的对应关系靠记忆。
- **CI 出产物但只做 workflow artifact（不发 Release）**：用户拿不到，等于没发布。
- **mac 只出 arm64**：Intel Mac 仍在使用；矩阵各出原生包成本可控（`fail-fast: false`）。
- **universal2 单包**：需要两套 `solidway` + app 二进制，`package-mac.sh` 要大改，收益仅是少一个文件。
- **不签名的 mac 产物**：违反 ADR 0011（禁 ad-hoc），Gatekeeper 直接拒绝，比构建失败更难排查。
- **Windows 只出便携 zip**：没有开始菜单/卸载项，对普通用户不友好；而只出安装包又会让「免安装试用」缺失，所以两者都出。
- **Windows 用 signtool + 证书**：需要 EV/OV 证书采购与 secrets 设计，当前无证书；先接受 SmartScreen 告警，后续另立 change。
- **用 `cargo tauri build --bundles dmg` 出 dmg**：会重建 `.app`，覆盖 post-copy 与签名结果（ADR 0011 已明确禁用）。

## 后果

### 正面

- 打一个 tag 即产出全部平台产物并挂到 Release，发布动作收敛为一件事；
- 版本漂移在构建前被拦住；
- 本机脚本仍是同一套（`package-*.sh`），CI 只做编排，本机与 CI 行为不易分叉。

### 负面 / 约束

- 签名私钥进入 GitHub Secrets（比 0016/0011 方案 B 的「不上云」放宽一步）；轮换证书时需同步更新 secrets；
- macOS 双架构 + 两个 `cargo install tauri-cli` 使一次发布的 CI 时长与额度明显高于普通 CI；
- `macos-13` 若在某天被 GitHub 下线，需要把 Intel leg 换成当时的 Intel 镜像（如 `macos-15-intel`）；
- 客户端包体积增加约 6–8 MB（自带网关二进制），换来「单装可用」；
- Windows 产物未签名：首次运行会被 SmartScreen 拦一次；便携包还要求系统已具备 WebView2（Win10 1803+ 通常随 Edge 安装）；
- Windows 安装包内 `solidway.exe` 的落点依赖 Tauri 的资源目录语义（Windows = 安装目录），首次发布需实机确认。
