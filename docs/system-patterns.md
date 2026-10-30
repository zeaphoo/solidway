# System Patterns — 工程模式事实快照

> 只记「我们怎么做」的事实，不记为何选它（动机在 `docs/adr/`）。新约定形成时更新本文件。

## 仓库组织

- Rust 代码统一放 `crates/` 目录，cargo workspace 管理（workspace 根为仓库根）。
- 前端独立在 `web/` 目录，与 Rust workspace 完全解耦。
- 数据库迁移文件放各 crate 的 `migrations/` 目录，采用 sqlx migration 方式管理；需要关闭外键重建表时使用 `-- no-transaction` 迁移并显式 `PRAGMA foreign_keys`。
- 软件发布前不保留升级迁移：主库与明细库各只保留一个 `0001_init.sql`，直接创建最终 schema；发布后再按增量迁移演进；合并迁移后旧的开发库需删除重建。
- 二进制 crate `solidway` 只负责解析 `data_root` 与进程生命周期；管理面 API 以库 crate `solidway-api`（目录 `crates/solidway_api`）提供，由二进制加载启动。
- 运行时数据（`solidway.db`、`master.key`、`solidway.log`）统一放在 `data_root` 目录下；**没有配置文件**，可调参数全在 `solidway.db` 的 `settings` 表。
- 桌面壳 `app/` 是独立 Tauri 2 cargo 项目，不进 Rust workspace（根 `Cargo.toml` `exclude = ["app"]`）；与 `web/` 互不依赖。
- Linux 服务通过 systemd 运行，安装包由 `scripts/package-linux.sh` 生成 deb / rpm；桌面应用仍由 Tauri bundler 单独打包（`scripts/package-linux-app.sh` / `scripts/package-mac.sh`），service 包与 app 包分离。
- **发布只由 `v*` 标签触发**（`.github/workflows/release.yml`）：先跑 `scripts/check-release-version.sh` 校验 tag 与全部清单版本一致，再产出 Linux 服务包 / Linux 客户端 / Windows 客户端 / macOS dmg 并挂到 GitHub Release；分支 push 不产出发布物（`ci.yml` 只做构建与测试）。取舍见 `docs/adr/0018-release-pipeline.md`。

## 管理面 API

- 所有管理 API 挂在 `/api` 前缀下，请求与响应为 JSON；错误统一为 `{ "error": { "code", "message" } }`。
- 认证使用服务端会话 + HttpOnly Cookie；口令用 Argon2id 哈希存储。
- 角色可见性口径：版本号走 `GET /api/version`（任意已认证角色）；`data_root` / 监听地址 / 静态资源来源等运行事实只走 `GET /api/system`、`GET /api/app/info`（仅 admin）。
- 路由为**用户私有资源**：`routes.owner_id` 绑定创建者，名称按 `(owner_id, name)` 唯一；路由、Provider、API Key、Provider 健康与普通用量查询都按 owner 隔离，admin 也不例外。admin 的跨用户能力只有「全局统计」只读聚合（`/api/admin/usage/*`）。
- admin 引导用**一次性引导令牌**：首启创建 `password_hash = "!"` 的锁定态 admin（任何口令都登不进），令牌只存**进程内存**的 SHA-256（不建表、不落盘）；明文只经 stdout 事件 `solidway: bootstrap token=… url=…` 交付（desktop 壳不写日志，standalone 由终端可见），拿它 `POST /api/auth/bootstrap` 设口令并当场签发会话。锁定态未解除时每次启动重新签发（自愈）。
- 引导只在 `standalone` 模式发生：`desktop` 模式不签发引导令牌，admin 允许以「未设置口令」状态存在，首启直接经桌面令牌进入（ADR 0017）。
- 口令的可选性只按**运行模式**区分，不引入账号级状态列：`standalone` 必须设口令；`desktop` 可无口令，此时锁定态 admin 无法口令登录（fail-closed），`GET /api/auth/me` 的 `has_password` 供界面区分「设置口令 / 修改口令」。
- 桌面令牌是**能力**：既能换取 admin 会话，也能在 `desktop` 模式下免当前口令设置 admin 口令（`POST /api/auth/password` 的 `desktop_token`，与 `/auth/desktop` 同源校验，成功写审计日志）。`standalone` 与非法令牌一律回落「必须校验当前口令」。
- 口令重置为 `solidway <data_root> --reset-password`：置回锁定态 + 清会话与令牌后退出，不打印任何凭证；新令牌由下一次启动签发。
- provider 凭据的 api_key 用 AES-256-GCM 加密落库：HKDF-SHA256 派生数据密钥、密文带 `kid` 版本前缀、AAD 绑定记录 id，格式 `sw:v1:<kid>:<iv>:<ct>:<tag>`；另存 `token_masked` 与 `token_hash`。
- 内置 provider 目录由构建期脚本 `scripts/gen-providers.mjs` 从 models.dev 生成，产物 `crates/solidway_api/src/provider/providers.json` 入库并编入二进制（运行时离线）；不建表、不写适配层。
- 目录只保留三种入站格式可直连的条目：生成器按 npm 包 / 覆盖表分类，无法映射的（Google 原生、Vertex 原生、Bedrock 原生、GitLab Duo）在生成期剔除；运行期再以 `default_api_formats` 非空 fail-closed。每条带 `kind`、`name_zh` 与可选 `summary` / `note`（i18n key）；`name` 为规范英文名与英文回退，控制台按语言选择名称，并在保存前说明计费形态与鉴权前提。

## 异步与阻塞边界

- 服务进程运行在 Tokio runtime；CPU 密集型操作（Argon2 口令哈希/校验）通过 `spawn_blocking` 执行，并用信号量限制并发，不在 async handler 中直接计算。
- 请求路径不做同步磁盘 I/O：静态资源磁盘模式使用 `tokio::fs::read`；TLS 证书/账户文件的读取与落盘（含 `fsync`）通过 `spawn_blocking` 执行。
- 用量聚合落盘按批放入单个 SQLite 事务；过期聚合/明细清理按固定批次删除，避免一次 `DELETE` 长时间持有写锁。
- `std::sync::RwLock` 只用于极短的内存快照/指标临界区，禁止跨 `.await` 持有；需要跨 await 的共享状态使用 `tokio::sync`。

## 数据面（转发）

- 数据面引擎独立为库 crate `solidway-core`：**纯内存**，不做数据库操作、不读配置文件、不提供管理端点。
- 控制面 `solidway-api` 持有 DB，负责下发 `CoreConfig`（含解密后的 provider api_key）与消费 `CoreEvent`（持久化 provider 健康与用量）。
- 寻址为 `API Key → 路由`；转发端点 `/v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/messages/count_tokens`、`/v1/models` 与 `/health` 同进程同端口。`/v1beta/*` 与 `/upload/*` 不再建模，保留路由挂载以返回 JSON 404。
- 入站按家族分流：OpenAI / Anthropic 共用 `/v1` 家族根；`base_url` 是上游 API 根，其中 `anthropic_messages` 族在缺版本段时由转发目标构造补 `/v1`（path 已含版本段则不补、不回写 `base_url`），OpenAI 族不补（见 `docs/adr/0008-version-segment-normalization.md`）。`/v1/models` 本地合成，只列出至少有一种入站格式可服务的模型。
- provider 凭据的 `models` 清单必填可空，**空清单 = 不服务任何模型**。
- 健康状态持久化于 `provider_health` 表，启动时**全量回灌**（含冷却截止）。
- 运行期参数归属：连接/首字节超时、冷却、`affinity_ttl`、会话 TTL 全在 DB settings（仅 admin）。地址与端口改动需重启；超时即时生效。
- 用量统计（模式 B）：聚合桶（小时/天 + 延迟直方图 + token）内存累加后定期落盘；事件分 `Access`（请求 / TTFB / bytes_in）与 `Completion`（outcome / duration / bytes_out / token）；逐条明细按 `usage.detail_mode` 保留；普通统计 `/api/usage/*` 按 owner 隔离，全局统计 `/api/admin/usage/*` 仅 admin。
- token 采集口径：上游响应带 `content-encoding` 时按 `gzip` / `deflate` / `br` / `zstd` **旁路解码**再解析 usage，转发字节与 `content-encoding` 头原样保留；OpenAI Chat 流式请求缺失 `stream_options` 时数据面补 `include_usage`（否则上游不回传 usage，token 恒 0）；解析不到即记 `usage_missing`，不估算。
- 统计时间桶按 `usage.timezone_offset_minutes` 的固定 UTC 偏移对齐；hour 保留 365 天、day 保留 730 天；不使用分钟桶。
- 运维：`provider_health` 可在界面查看（冷却/退役 + 原因）；仅 admin 可人工解除标记（`DELETE /api/provider-health/{id}`，删行后即时下发）。

### api ↔ core 交互模型

- 依赖单向：`solidway` → `solidway_api` → `solidway_core`；core 不依赖 api，也没有任何对 api 的回调机制（core 对 api 零感知）。
- core 的对外接口只有四个入口：`Core::new`（按 `CoreConfig` 建实例）、`apply_config`（全量原子替换，失败保持旧配置）、`subscribe`（事件广播接收端）、`router`（转发端点 + `/health`）。
- 配置自上而下：api 在启动时与管理面每次写操作后（`core_bridge.rs` 的 `sync_core`）从 DB 拼出 `CoreConfig` 调 `apply_config`；无「热加载」概念。
- 事件自下而上但非调用：core 经 broadcast 广播 `CoreEvent`（Access / Attempt / HealthChanged），**缺订阅者即丢弃**（不阻塞、不缓存）；api 的常驻 event pump 消费事件并落库。
- 请求路径：`/v1/*`、`/health` 由 core 的 router 直接应答，**不经过 api 的 handler**（启动时 `api.router().merge(core.router())` 同端口合并）；api 仅在请求结束后经事件流得知结果。

## 前端

- 前端为纯静态 SPA，构建产物由 `solidway` 同进程托管（单二进制），未知路径回退 `index.html`。
- 所有用户可见文案走 i18n（`zh-CN` / `en-US`），不硬编码。
- 支持浅色 / 深色主题，以 `documentElement` 上的 `.dark` 类切换，首访取系统偏好并持久化。
- 服务端数据用 react-query，UI 态用 zustand，职责不重叠。
- **引导 / 帮助**：「快速开始」（`/welcome`）的进度一律由接口数据推导（`web/src/lib/onboardingSteps.js` 是唯一的步骤事实来源），不在本机记进度；本机 `localStorage` 只存按用户隔离的「已跳过 / 已完成」意图标记（`solidway-onboarding:v1:<userId>`）。自动进入闸门在 `AppShell`，每次加载页只判一次并 `replace`，不做路由守卫。
- **帮助正文与 UI 文案分文件**：`locales/<lang>.json` 放界面文案，`locales/help/<lang>.json` 放帮助正文与检索关键词，`i18n.js` 合并为 `help` 命名空间；帮助分区与锚点结构在 `web/src/lib/helpContent.js`。
- **示例代码唯一来源**：`web/src/lib/accessSnippets.js` 生成 curl / SDK / 终端 Agent 片段，路由详情页对话框与帮助中心共用 `AccessGuidePanel`，禁止在页面内拼示例字符串。接入地址归一化（`buildBaseUrls`）与「按凭据推导可用格式 / 模型」（`formatsAndModels`）也只此一份。
- **跨页直达用 query 参数**：目标页识别 `?new=<what>` 打开对应对话框后立即 `setSearchParams(..., { replace: true })` 清理，避免刷新重开（`/routes/:id?new=provider`、`/api-keys?new=1`）。
- **admin-only 接口在前端必须显式关请求**：`useAppInfo` / `useSettings` / `useUsers` 均接受 `enabled`，非 admin 传 `false`，不靠 403 报错兜底。
- **页面只读、编辑走对话框**：设置类页面（如「个人」）只展示只读信息与入口按钮，编辑动作用 `components/<domain>/*Dialog.jsx` 完成；对话框在打开时以当前值重置，取消 / 关闭 MUST NOT 提交，提交成功即关闭并刷新受影响缓存。
- **会话来源判别**：桌面免登录会话以 `sessionStorage` 里的桌面令牌为唯一判据（`web/src/lib/desktopLogin.js::isTokenSession()`）。令牌只在 `/desktop` 换会话与 `RequireAuth` 续会话时写入，唯一清除点是显式退出登录，因此「有令牌」⟺「当前会话由令牌签发」。页面 MUST NOT 用 `isDesktopShell()`（localStorage）推断会话来源——桌面壳内以口令登录的用户会被误判。
- 前端只与 `/api/*` 管理端点交互，不感知数据面；`/v1/*` 与 `/health` 面向 LLM 客户端（脚本 / opencode / pi 等），三类入口同端口合并、各自由 api / core 应答。

## 桌面壳（app/）

- 职责单一：定位并启动 `solidway <data_root>`、轮询健康、在 webview 中打开控制台、退出时清理子进程。
- 逻辑全在 Rust；界面只有一个静态 splash / 错误页，文案由 Rust 侧经 `window.solidwaySetStatus` 注入（页面加载完成后重注入一次，避免丢失）。
- **GTK 线程约束**：窗口创建/操作与子进程派生只在主线程执行；后台监控线程通过 `run_on_main_thread` 投递。
- 端口不来自配置文件：子进程 bind 成功后输出 `solidway: listening addr=…`，壳据此得知地址并导航；stdout/stderr 追加写入 `<data_root>/solidway.log`。
- 引导事件例外：`solidway: bootstrap …` 行含明文令牌，壳**不写入** `solidway.log`，只解析进内存；窗口地址直接用事件里的 `url`，不在壳里拼令牌。
- 收缩/失败：壳收到 `bind-failed`（含占用者 pid/comm）即展示错误页，不重试；收到 `restart-requested` 则**无条件**立即重启（不走退避、不计失败次数）。
- 孤儿防护：壳以 `Stdio::piped()` 提供 stdin 并只保持打开；`solidway --mode desktop` 读到 stdin EOF 即退出（跨平台，无轮询、无内核特性）。
- 配置事实：`data_root` 取平台应用数据目录，不提供覆盖环境变量。
- 生命周期：关窗是否退出由偏好「关闭窗口时最小化到托盘」决定（默认开启 = 常驻托盘）；退出清理覆盖托盘退出、信号（`SIGTERM`/`SIGINT`/`SIGHUP`）与父进程被杀（Linux `PR_SET_PDEATHSIG`）。
- 子进程异常退出后固定自动重启（退避重试 + 次数上限），不提供开关环境变量。
- **桌面偏好**：`<data_root>/desktop.toml`（`autostart` / `close_window_to_tray`）由壳唯一写、启动时物化；`solidway` 只读它来渲染控制台。不变式：文件内容 == 壳实际行为，执行失败不写新值。登录自启在 debug 构建默认关闭、发布构建默认开启；关窗最小化两种构建都默认开启。
- **控制台编辑链路**：控制台 `/system` → `PUT /api/app/desktop`（admin、desktop 模式）→ 服务发 `solidway: desktop-apply id=… autostart=… close_to_tray=…` → 壳执行 → 壳写 `<data_root>/desktop.status.json` 回执 → 控制台按 `apply_id` 确认/回滚。服务只发事件、不写偏好文件。
- **托盘兜底**：托盘保留「开机自启动」「关闭窗口时最小化到托盘」两个勾选项，与控制台共用同一执行管线（服务不可用时仍可改）。
- **托盘可用性与 Dock 重开**：托盘「打开控制台」**始终可用**（点击按状态分流：有窗口前置 / 无窗口重建 / 未就绪走启动页与状态分流，不覆盖已有失败原因；不做置灰——Linux 托盘 `set_enabled` 不可靠且置灰锁死主入口）；Dock 图标存在性单独跟随控制台窗口存在性（`regular` / `accessory`，创建与销毁两时机同步；splash / notice 窗口不计入；恒隐藏 `LSUIElement` 被否）；macOS Dock 点击（`RunEvent::Reopen`，Tauri 里是 macOS-only 变体，处理臂 `cfg` 门控）与托盘「打开控制台」同一打开路径。语义取舍见 `docs/adr/0018-tray-menu-dock-reopen.md`（含 2026-10-27 置灰→始终可用修订）。
- **自启动与单实例**：登录自启经 `tauri-plugin-autostart`（Linux XDG / macOS LaunchAgent / Windows 当前用户 `Run`）注册，命令固定携带 `--hidden`；`tauri-plugin-single-instance` 保证第二实例在派生 `solidway` 前退出。
- **托盘点击的平台差异**：Windows 派发 `Click` / `DoubleClick`（左键单击即打开控制台、不弹菜单，双击同样接受、幂等；右键出菜单）；macOS 只有左键 `Click`（默认出菜单）；Linux 的 gtk/ksni 后端不派发任何点击事件。macOS/Linux 必须以菜单项作为入口，MUST NOT 假设点击事件跨平台可用。
- **网关二进制落点**：客户端包自带 `solidway`（Linux/Windows 用 `bundle.resources`，mac 用 post-copy 进 `Contents/MacOS`）；壳的查找顺序是「可执行文件同目录 → `resource_dir()` → 开发期 target（仅 debug）→ PATH」，MUST NOT 由壳拼字符串猜打包器布局。Linux 客户端 deb 包名为 `solidway-app`，与服务包 `solidway` 区分。
- **静默启动**：`--hidden` 下不主动建 splash / 控制台，状态只在托盘；失败路径（含握手超时、端口冲突、子进程退出）仍展示错误页。
- **桌面免登录**：服务每次 desktop 启动签发进程级令牌（`desktop-login` 事件，壳不落日志），壳把 webview 导航到 `/desktop#<token>` 换 admin 会话；进程内可复用、重启轮换。admin 锁定态拒绝，首启仍走引导设口令。「退出登录」保留（切用户），显式退出清本地令牌；回到 admin 走托盘「以管理员自动登录」。取舍见 `docs/adr/0016-desktop-auto-login.md`。

## Provider 配置

- **预设即端点来源**：预设条目（`provider` 命中目录）的 `base_url` / `api_formats` / `auth_scheme` 由目录决定，控制台只读；唯一可填的是端点模板变量。差异一律通过「复制为自定义」（`<key>-custom`）表达，不做"偏离检测 + 自动改名"。
- **派生字段写入时冻结**：`base_url`（模板展开）、`api_formats`、`auth_scheme`、`options`（`path_template` / `path_params` / `base_url_vars`）在写入时一次算好落库，运行期只读记录、不查目录、不按字符串前缀推断行为。
- **模板原语唯一**：`crates/solidway_api/src/provider/template.rs` 的 `expand` / `match` 是端点相关的唯一判断入口（一致性校验、变量反推、Google 端点判定、是否仍符合预设）；前端副本在 `web/src/lib/providerEndpoint.js`，两侧规则一致并有表驱动测试。
- **上游路径由模板渲染**：预设声明 `path_template`（`{param}` / `{model}` / `{action}` / `{endpoint}`），路径参数随凭据冻结；客户端入站路径保持固定，不含 project/location/deployment 等凭据信息——这是 key 池化能换号的前提。
- **签名与地址同源**：`sigv4` 在最终请求体与最终 URL 确定后签名（含探针），`region` / `aws_service` / `anthropic_version` 由预设冻结到 `options`；事件流响应（`application/vnd.amazon.eventstream`）按流式透传、不解析 usage。
- **不可探测不是健康**：Service Account、空模型清单、空 `api_formats` 的凭据不参与探针，界面显示"不可探测"并给出原因。

## 测试

- 测试等待一律用**可观测条件 + 显式预算**（事件订阅 / `hit_count` / `probe_count` / 快照轮询），禁止用固定 `sleep` 拼时序；预算耗尽即失败并打印观测值，禁止「重试直到过」式假修复（2026-10-21 起，probe-body-timeout-deflake 确立）。
- 断言不放宽、不删除；需要更强判据时**新增**断言（如区分探针请求与转发请求的 body 标记），不改写既有断言语义。
- mock 上游为真实 socket + tokio timer 混合；测试不用 `tokio::time::pause`（auto-advance 会与真实 IO 交互扭曲时序语义，见 `openspec/changes/probe-body-timeout-deflake/design.md` D2）。

## 协作流程

- 业务行为变更必须走 OpenSpec proposal（`openspec/changes/<change-name>/`），确认后才开发。
- Git 分支模型：**main 单主干**（2026-10-15 起；develop 已被移植吸收并废弃，feature 分支随完结删除）。
- 已推 GitHub（私有，2026-10-11 首推）。
