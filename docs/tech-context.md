# Tech Context — 技术栈事实状态

> 记录「我们用什么、跑在哪」，偏静态技术构成。

## 后端

- 语言：Rust（cargo 1.97.1），workspace 根 `Cargo.toml`。
- Web 服务：axum 0.8（最新版，升级前与主管确认）。
- 数据库：SQLite，经 sqlx 0.9 连接与管理；迁移用 sqlx migration。两个文件：`<data_root>/solidway.db`（配置 / 会话 / 健康 / 聚合桶，`migrations/`）与 `<data_root>/usage_detail.db`（逐条请求明细，`migrations_detail/`）；均开 WAL，明细走内存缓冲每 2.5s 单事务批量写。
- 运行时：tokio 1.x。
- 认证：Argon2id 口令哈希（`argon2`）；服务端会话 + HttpOnly Cookie。
- 凭据加密：AES-256-GCM + HKDF-SHA256（`aes-gcm` / `hkdf`），密文格式 `sw:v1:<kid>:<iv>:<ct>:<tag>`。
- 运行时配置：**数据库 `settings` 表**（`network` / `auth` / `gateway` / `usage` / `access` / `tls` 六节）；`usage.timezone_offset_minutes` 为只读统计时区（首次初始化取操作系统本地 UTC 偏移，无法判定时回退 UTC），用于对齐 hour/day 桶；不再有配置文件。主密钥 `<data_root>/master.key`（32 字节，Unix `0600`），可由 `--master-key <hex|base64>` 临时覆盖。
- provider 目录：构建期由 `scripts/gen-providers.mjs` 从 models.dev 生成，产物入库（`crates/solidway_api/src/provider/providers.json`），运行时离线。
- 数据面：`solidway-core`（纯内存转发引擎，依赖不含 sqlx）；上游 HTTP 用 `reqwest`（rustls + stream）。
- 转发参数：连接/首字节超时在 DB `network`（保存后即时生效）；冷却与 `affinity_ttl` 在 DB `gateway`（仅 admin）。
- 入站协议家族：OpenAI（`/v1/chat/completions`、`/v1/responses`）、Anthropic（`/v1/messages`、`/v1/messages/count_tokens`）；不做跨协议翻译。Gemini 入站已移除，地址语义与家族范围见 `docs/adr/0003-inbound-families-and-base-url.md` 与 `docs/adr/0013-drop-gemini-inbound.md`。
- 内置 HTTPS：`tls` 设置节（`settings` 表第六节）；`rustls` + `tokio-rustls` 终止 TLS（h2 / http/1.1），`instant-acme` 走 ACME（`http-01` 默认 / `tls-alpn-01` 可选），`rcgen` 生成 ALPN-01 临时挑战证书；账户与证书缓存在 `<data_root>/tls/`（摘要命名、0600、原子写），续期到期前 30 天、退避 5 分钟→1 小时、24 小时 5 次失败后暂停。选型见 `docs/adr/0002-tls-acme-library.md`。

## 前端

- 目录：`web/`
- 包管理：pnpm
- 构建：vite
- 框架：react（JS，不用 TS）
- 样式：tailwind + shadcn/ui
- 状态管理：zustand
- 前端路由：react-router（v7）
- 服务端状态：@tanstack/react-query（v5）
- 表单校验：react-hook-form + zod
- 国际化：i18next + react-i18next（`zh-CN` / `en-US`，默认中文）
- 提示：sonner
- 图表：recharts（`UsageTrendCharts` 按需加载，页面本身也走 `React.lazy`）

## 桌面应用

- 目录：`app/`；Tauri 2（`tauri` 2.x，feature `tray-icon`）。
- 独立 cargo 项目：不进 `crates/` workspace（根 `Cargo.toml` 有 `exclude = ["app"]`）。
- 纯 Rust：无前端框架/打包器；界面只有 `app/ui/index.html`（内联 CSS 的 splash / 错误页）。
- 依赖：`tauri` / `serde` / `serde_json` / `toml` / `tauri-plugin-autostart` / `tauri-plugin-single-instance`；Unix 下加 `libc`（信号处理、`PR_SET_PDEATHSIG`）。
- Linux 登录项路径由 `tauri-plugin-autostart` 固定为 `~/.config/autostart/Solidway.desktop`（不读 `XDG_CONFIG_HOME`），`Exec=` 为「可执行文件 + `--hidden`」。
- 桌面免登录：desktop 模式启动时生成进程级令牌（内存只存 SHA-256），bind 后经 stdout `solidway: desktop-login url=http://<loopback>/desktop#…` 交付（固定本机回环，TLS/对外服务时也不走公网域名）；壳打开 `/desktop#<token>` 换会话，进程内可复用、重启轮换；admin 锁定态（未设口令）拒绝。
- 壳级文件（均在 `data_root`）：`desktop.toml`（偏好，壳唯一写者）、`desktop.status.json`（最近一次 apply 回执，壳写服务读）、`desktop.log`（壳级日志，与子进程 `solidway.log` 分开）。
- 图标：由 `web/public/favicon.svg` 经 `cargo tauri icon` 生成到 `app/icons/`。
- 健康探测：自实现的本地回环最小 HTTP GET（避免为本地探测引入 async HTTP 栈）。

## 运行时接口

- CLI：`solidway <data_root> [--mode desktop|standalone] [--listen <addr>] [--master-key <hex|base64>] [--web-assets <dir>]`。
- 日志语言：`tracing` 日志文案为英文（便于贴 issue / 与厂商沟通），UI 与文档保持中文；stdout 事件契约单独见下条。
- stdout 事件契约（机器可读，ASCII、不经 `tracing`）：`solidway: listening addr=…` / `bind-failed …` / `restart-requested …` / `desktop-apply …`（仅 desktop，由 `PUT /api/app/desktop` 触发）/ `desktop-login url=…`（仅 desktop，含令牌、壳不写日志）/ `bootstrap-consumed`（引导令牌已消费）。
- `--listen` 优先级最高（不写回 DB），接受 `host:port` / `:port` / 裸端口；不允许端口 0。
- 端口占用者识别用 `listeners` crate（Linux `rustix` / macOS `byteorder` / Windows `windows`，无 bindgen）。
- 前端资产：`--web-assets`（两种构建都生效）> debug 读仓库 `web/dist`（编译期绝对路径）> release 内置（`rust-embed`）。

## 平台支持

| 能力 | Linux | Windows | macOS |
|---|---|---|---|
| 管理面 / 数据面 / SQLite / Argon2 / AES-GCM | ✅ | ✅ | ✅ |
| 内置 TLS / ACME（rustls + instant-acme） | ✅ | ✅ | ✅ |
| 前端静态托管（内置 / 磁盘） | ✅ | ✅ | ✅ |
| 端口占用者识别 | ✅ | ✅ | ✅ |
| 托养方消失即退出（stdin EOF 看门狗） | ✅ | ✅ | ✅ |
| 登录自启动（XDG / LaunchAgent / Run 键） | ✅ | ✅（待实测） | ✅（待实测） |
| 直接终止信号清理 | SIGTERM/INT/HUP | 靠 `RunEvent::Exit` | SIGTERM/INT/HUP |
| 主密钥权限 | `0600` | 依赖目录 ACL | `0600` |

- Windows 上 `<data_root>` **不得**位于共享目录或盘符根目录（主密钥依赖目录 ACL）。
- CI：`.github/workflows/ci.yml` 三平台跑 `cargo test --workspace`，外加 `pnpm build`。

## 协作设施

- OpenSpec 1.x（`openspec/` 目录，skills 在 `.agents/` 并入库）。

## 部署假设

- 单二进制形态：`solidway` 同时提供 `/api` 与托管前端构建产物（`web/dist`，SPA fallback）；开发期用 vite 代理 `/api`。
- 桌面形态：`app/` 启动 `solidway` 子进程并把其 cwd 设为含 `web/dist` 的目录，再在 webview 中打开 `http://127.0.0.1:<port>/`。
- 重启连续性：Provider 健康走种子回灌；选号指标由 `usage_attempt_bucket` 聚合最近 30 分钟后下发；会话粘性在收尾时写入单行表 `runtime_snapshot` 并在启动时作为种子恢复；SIGTERM/SIGINT 与桌面重启走统一收尾（落盘用量 + 写快照，总预算 5s）。取舍见 `docs/adr/0005-restart-continuity.md`。
- 内置 HTTPS 的部署与运维（DNS、端口映射与权限、staging→生产、备份与排障）见 `docs/https-deployment.md`。
- 发布与签名流水线：**推送 `v*` 标签即出全部产物**（`.github/workflows/release.yml`）——Linux 服务包 deb/rpm（`scripts/package-linux.sh`）、Linux 桌面客户端 deb/AppImage（`solidway-app_<version>_<arch>.*`）（`scripts/package-linux-app.sh`，自带网关二进制，`bundle.resources` 落 `/usr/lib/<产品名>/`；deb 包名 `solidway-app` 以区别于服务包）、Windows 客户端 NSIS/MSI 安装包 + 便携 zip（`scripts/package-windows.sh`，`solidway.exe` 经 `bundle.resources` 装到应用同目录，未做代码签名）、macOS 自签 `Solidway.app` + `solidway-app_<version>_<arch>.dmg`（`scripts/package-mac.sh`，post-copy `solidway` 进 `Contents/MacOS/`、内层先签外层后签，x86_64 / arm64 各一份），全部挂到 GitHub Release。版本一致性由 `scripts/check-release-version.sh` 在构建前强制校验（tag 必须等于 `v<version>`）；mac 签名需仓库 secrets（p12 + 口令），缺失即失败、禁 ad-hoc、不做 Apple 公证。分支 push 不产出发布物（`ci.yml` 只做构建与测试）。取舍见 `docs/adr/0011-mac-desktop-packaging-and-signing.md`、`docs/adr/0018-release-pipeline.md`。
