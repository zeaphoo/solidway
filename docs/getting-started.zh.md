<div align="right">

[English](getting-started.md) · **中文**

</div>

# 快速开始

如何构建、运行与部署 Solidway。它做什么见 [README](../README.zh.md)。

## 环境要求

- Rust stable（workspace 使用 edition 2024）
- Node.js 22+ 与 pnpm 10（用于控制台）
- Linux 上仅**桌面端**需要 `pkg-config` 与 `libwebkit2gtk-4.1-dev`；服务端二进制除 glibc 外没有系统依赖。

## 从源码构建

release 构建会把控制台嵌入二进制，所以先构建控制台：

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir web build

cargo build --release -p solidway
```

产物是 `target/release/solidway`。

## 运行

```bash
./target/release/solidway ./data --mode standalone
```

首次启动会：

1. 创建 `./data/`；
2. 打印一次性引导令牌；
3. 监听 `127.0.0.1:6565`。

```text
solidway: listening addr=127.0.0.1:6565
solidway: bootstrap token=<token> url=http://127.0.0.1:6565/bootstrap#<token>
```

打开该地址、设置管理员口令，之后都在控制台里完成：创建路由 → 添加 Provider 凭据 → 创建 API Key → 把 SDK 指向 `http://<host>:6565/v1`。

常用参数：

| 参数 | 含义 |
|---|---|
| `--mode standalone` \| `desktop` | desktop 模式允许重启请求，并在 stdin 关闭时退出 |
| `--listen <addr>` | 本次运行临时覆盖绑定地址与端口（不持久化） |
| `--web-assets <dir>` | 从磁盘目录加载控制台，而不是用内嵌副本 |
| `--reset-password` | 把管理员打回「未设置口令」状态后退出 |
| `--master-key <hex\|base64>` | 仅用于恢复时直接提供主密钥 |

`solidway --help` 会打印同样的列表。

## Linux 部署

```bash
./scripts/package-linux.sh        # -> dist/linux/*.deb, *.rpm
```

安装包会安装：

- `/usr/bin/solidway`
- `/usr/lib/systemd/system/solidway.service`（系统用户 `solidway`，数据目录 `/var/lib/solidway`，为 80/443 端口授予 `CAP_NET_BIND_SERVICE`）
- `/var/lib/solidway` 作为数据根目录

二进制动态链接 glibc，因此请在你打算支持的**最老发行版**上构建。

## 桌面端

Tauri 外壳：在本机托管网关、用 webview 打开控制台，支持开机自启与托盘常驻。覆盖 Linux、Windows、macOS。

构建方式：

```bash
./scripts/package-linux-app.sh     # deb + AppImage
./scripts/package-windows.sh       # NSIS/MSI + 便携 zip
./scripts/package-mac.sh           # .app + .dmg（自签）
```

每个安装包都把网关二进制放在应用可执行文件同目录，单装即可运行。macOS 为自签、未公证；Windows 未签名——首次运行会遇到 Gatekeeper / SmartScreen 提示。

## 数据目录

`<data_root>` 是唯一的状态目录：

| 平台 | 路径 |
|---|---|
| Linux | `~/.local/share/xyz.solidway` |
| macOS | `~/Library/Application Support/xyz.solidway` |
| Windows | `%APPDATA%\xyz.solidway` |
| Linux 安装包 | `/var/lib/solidway` |

| 文件 | 内容 |
|---|---|
| `solidway.db` | 用户、路由、Provider 凭据（加密）、设置、会话 |
| `usage_detail.db` | 逐条用量明细（保留策略可配置） |
| `master.key` | Provider 凭据的加密主密钥——**必须单独备份** |
| `solidway.log` | 网关 stdout/stderr（仅桌面端） |

没有 `master.key`，`solidway.db` 里的 Provider 凭据无法解密。两个文件要一起备份，且都不要进版本库。

## TLS

支持两种形态：

1. **内置 ACME** —— 在控制台「系统 → HTTPS」配置公网域名；Solidway 用 `http-01` 或 `tls-alpn-01` 申请并续期证书，需要 80/443 端口。
2. **反向代理** —— 在上游终止 TLS，Solidway 只监听回环；转发原始 `Host` 与客户端 IP 头。

## 常用运维命令

```bash
# 重启服务
sudo systemctl restart solidway

# 跟踪网关日志
journalctl -u solidway -f

# 重置管理员口令（回到未设置状态，之后在控制台重设）
sudo -u solidway /usr/bin/solidway /var/lib/solidway --reset-password
```
