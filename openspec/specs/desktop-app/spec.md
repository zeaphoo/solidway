# desktop-app Specification

## Purpose
TBD - created by archiving change add-desktop-app. Update Purpose after archive.

## Requirements

### Requirement: 桌面壳为纯 Rust

桌面应用 MUST 以 Rust 实现全部逻辑（进程监督、健康监控、窗口与托盘）；MUST NOT 引入前端框架或打包器。

界面 MUST 只包含一个极简 splash / 错误页（纯 HTML，无框架），用于在 solidway 就绪前展示状态。

#### Scenario: 无前端框架
- **WHEN** 检查 `app/` 的依赖与资源
- **THEN** 不存在前端框架、打包器或构建产物

### Requirement: 启动 solidway

应用 SHALL 以**唯一参数 `data_root`** 启动 `solidway` 子进程。

`data_root` MUST 取平台应用数据目录，MUST NOT 依赖应用专属环境变量覆盖。`solidway` 二进制的定位顺序 MUST 为：应用可执行文件同目录 → 开发期工作区构建产物目录 → `PATH`。

子进程异常退出后 MUST 按退避策略自动重启；MUST NOT 依赖环境变量开关。子进程的 stdout/stderr MUST 追加写入 `<data_root>/solidway.log`。

#### Scenario: 首次启动
- **WHEN** 应用启动且 `data_root` 尚无数据库
- **THEN** 应用创建目录并启动 `solidway`，由其对数据库执行迁移与 admin 引导

#### Scenario: 找不到二进制
- **WHEN** 上述定位顺序均未找到 `solidway`
- **THEN** 应用 MUST 展示可读错误（含已尝试的路径），SHALL NOT 静默失败

#### Scenario: 控制台静态托管可用
- **WHEN** 子进程启动完成且 `web/dist` 位于仓库根或应用可执行文件附近
- **THEN** `GET /` 返回控制台页面，而非 404

#### Scenario: 子进程异常退出
- **WHEN** `solidway` 非正常退出且未超过重启上限
- **THEN** 应用按退避策略自动重启子进程

### Requirement: 健康监控与状态

应用 SHALL 周期探测 solidway 健康（`GET /api/health`），并在窗口与托盘展示当前状态（启动中 / 运行中 / 已停止 / 异常）。

探测端口 MUST 从 `<data_root>/config.toml` 的 `[server] port` 读取；文件缺失或解析失败时回退默认 `6565`。

子进程提前退出时 MUST 被检出并反映到状态中；子进程存活但超时（默认 90 秒）仍不健康时 MUST 进入失败态并给出日志路径。

#### Scenario: 就绪
- **WHEN** solidway 健康探测成功
- **THEN** 状态变为「运行中」，窗口导航到其本地端口

#### Scenario: 子进程退出
- **WHEN** solidway 子进程意外退出
- **THEN** 状态变为「已停止」并展示于窗口 / 托盘

#### Scenario: 端口来自配置
- **WHEN** `<data_root>/config.toml` 配置 `[server] port = 7000`
- **THEN** 应用探测并打开 `127.0.0.1:7000`

#### Scenario: 启动超时
- **WHEN** 子进程仍在运行但超过 90 秒未通过健康检查
- **THEN** 应用终止该子进程并展示失败原因与日志路径

### Requirement: Webview 打开本地端口

solidway 就绪后，应用 SHALL 在 webview 中打开 `http://127.0.0.1:<port>/`（由 solidway 托管的控制台）。

#### Scenario: 显示控制台
- **WHEN** 健康探测成功
- **THEN** 主窗口展示 solidway 控制台页面

### Requirement: 托盘菜单

应用 SHALL 提供托盘图标与菜单，至少包含：当前状态（只读项）、打开控制台、重启 Solidway、退出。

「重启」MUST 停止当前子进程后重新启动并重新进入健康轮询。

#### Scenario: 从托盘打开控制台
- **WHEN** 用户点击托盘「打开控制台」
- **THEN** 主窗口显示并定位到 solidway 控制台

#### Scenario: 从托盘重启
- **WHEN** 用户点击托盘「重启 Solidway」
- **THEN** 旧子进程被停止，新子进程启动，状态回到「启动中」并在就绪后转「运行中」

### Requirement: 常驻托盘

关闭控制台窗口的行为 SHALL 由偏好「关闭窗口时最小化到托盘」决定：

- 开启（默认）时，关闭窗口 MUST NOT 结束应用：应用常驻托盘，可通过托盘重新打开控制台。
- 关闭时，关闭最后一个窗口 SHALL 结束应用，并按「退出清理」终止其启动的 `solidway` 子进程。

托盘「退出」在两种设置下 MUST 都结束应用及其子进程。

#### Scenario: 关闭控制台窗口
- **WHEN** 偏好为开启（默认），用户关闭控制台窗口
- **THEN** 应用与 `solidway` 子进程继续运行，托盘仍可打开控制台

#### Scenario: 取消勾选后关闭控制台窗口
- **WHEN** 偏好为关闭，用户关闭控制台窗口
- **THEN** 应用退出，`solidway` 子进程被终止，监听端口被释放

#### Scenario: 从托盘退出
- **WHEN** 用户在任意偏好下点击托盘「退出」
- **THEN** 应用退出且不遗留子进程

### Requirement: 退出清理

应用退出时 MUST 停止其启动的 `solidway` 子进程，SHALL NOT 遗留孤儿进程。

清理 MUST 覆盖：托盘「退出」、窗口关闭导致的退出、以及进程收到 `SIGTERM`/`SIGINT`/`SIGHUP`；父进程被 `SIGKILL` 或崩溃时 MUST 由内核兜底结束子进程（Linux `PR_SET_PDEATHSIG`）。

> 因 GTK 限制，子进程 MUST 在主线程派生（`PR_SET_PDEATHSIG` 绑定父线程）。

#### Scenario: 退出应用
- **WHEN** 用户退出应用
- **THEN** `solidway` 子进程被终止

#### Scenario: 收到终止信号
- **WHEN** 应用进程收到 `SIGTERM`
- **THEN** 子进程先被终止，应用再退出

#### Scenario: 父进程被强杀
- **WHEN** 应用进程被 `SIGKILL`
- **THEN** `solidway` 子进程随之退出，不占用端口

### Requirement: 失败自动重启（可开关）

应用 SHALL 提供「子进程失败自动重启」开关，默认**关闭**；开启时 MUST 以退避策略重试并设次数上限。

#### Scenario: 默认不自动重启
- **WHEN** 开关关闭且子进程退出
- **THEN** 应用不重启，仅更新状态

#### Scenario: 开启后退避重试
- **WHEN** 开关开启且子进程退出
- **THEN** 应用按退避间隔重启，超过上限后停止尝试并标记异常

### Requirement: stdout 事件契约

服务 MUST 向 stdout 输出以下机器可读事件行，前缀固定为 `solidway: `，格式为「事件名 + 若干 `k=v`」，ASCII 且不本地化：

- `solidway: listening addr=<host:port>`
- `solidway: bind-failed addr=<host:port> kind=<reason> [holder_pid=<n>] [holder_comm=<name>]`
- `solidway: restart-requested reason=<reason>`
- `solidway: desktop-apply id=<8hex> autostart=<0|1> close_to_tray=<0|1>`（仅 desktop 模式，由控制台的偏好请求触发）

事件行 MUST NOT 受 `RUST_LOG` 过滤影响。`bind-failed` 后进程 MUST 退出。

#### Scenario: 端口被占用
- **WHEN** 监听端口被其他进程占用
- **THEN** 输出 `bind-failed`（尽量含 `holder_pid` / `holder_comm`），进程以非零码退出，且不输出 `listening`

#### Scenario: 日志级别不影响事件
- **WHEN** 以 `RUST_LOG=error` 启动
- **THEN** `listening` 事件仍然输出

#### Scenario: 控制台修改桌面偏好
- **WHEN** 控制台调用 `PUT /api/app/desktop`
- **THEN** 服务在该请求处理中输出一行 `desktop-apply`，且 standalone 模式不输出该事件

### Requirement: 桌面壳发现端口

桌面壳 MUST 通过子进程 stdout 的 `listening` 事件获得实际监听地址；MUST NOT 读取 `config.toml` 或任何配置文件来推导端口。

握手等待 MUST 有超时；子进程在握手前退出时 MUST 直接展示错误，不再等待健康检查。

#### Scenario: 就绪后打开控制台
- **WHEN** 收到 `listening addr=127.0.0.1:6565` 且健康探测通过
- **THEN** 主窗口打开 `http://127.0.0.1:6565/`

#### Scenario: 端口冲突报错
- **WHEN** 收到 `bind-failed`
- **THEN** 展示含占用者信息的错误页，不重试、不更换端口

### Requirement: 主动重启无条件执行

桌面壳收到 `restart-requested` 事件时 MUST 立即重启子进程，MUST NOT 应用失败退避策略，也 MUST NOT 计入失败重启次数。

#### Scenario: 界面触发重启
- **WHEN** 用户在设置页点击「立即重启服务」
- **THEN** 服务退出，桌面壳立即重启它，并在新端口就绪后重新导航

### Requirement: Dock 重开控制台

macOS 上触发的重开请求（`RunEvent::Reopen`，如点击 Dock 图标、Finder 重新打开应用）SHALL 复用「打开控制台」的打开路径：控制台窗口存在时 MUST 显示并聚焦，不存在时 MUST 重建控制台窗口并进入与托盘「打开控制台」相同的目标地址（按既有导航优先级，desktop 模式下为桌面登录地址）。

窗口已存在时 MUST 只显示与聚焦，MUST NOT 导航、刷新或重建窗口。

该请求仅由 macOS 投递（`applicationShouldHandleReopen`）；其他平台 MUST NOT 因本需求产生任何行为变化。Dock 图标的显隐由「Dock 图标存在性」决定：窗口不存在时图标隐藏，此时重开请求仅可能来自 Finder / `open -a` 等系统路径，仍按本要求处理。

#### Scenario: 窗口已存在时点击 Dock 图标
- **WHEN** 控制台窗口存在（含最小化），用户点击 Dock 图标
- **THEN** 窗口显示并获得焦点，内容不导航、不刷新、不重建

#### Scenario: 重开请求且窗口不存在（兕底）
- **WHEN** 控制台窗口不存在，macOS 投递重开请求（如 Finder / `open -a` 重新打开应用）
- **THEN** 控制台窗口重建，并进入与托盘「打开控制台」相同的目标地址，Dock 图标随之恢复可见

#### Scenario: 非 macOS 平台行为不变
- **WHEN** 应用运行在 Linux / Windows
- **THEN** 不存在 Dock 重开路径，托盘菜单与关窗行为与本需求引入前一致

### Requirement: 打开控制台菜单项可用性

托盘「打开控制台」MUST 跟随控制台窗口存在性更新启用状态：窗口不存在时置灰（保留菜单项、不可点），窗口创建成功后恢复可用，窗口销毁后重新置灰。

窗口存在性 MUST 以控制台窗口为唯一判据，splash、notice 等瞬态窗口不计入；启用状态 MUST 在窗口创建与销毁两个时机更新。

置灰语义：窗口不存在时「打开控制台」与「以管理员自动登录」同义（重建窗口并建立 admin 会话），故只保留后者可用，MUST NOT 删除该菜单项。

#### Scenario: 窗口不存在时置灰
- **WHEN** 控制台窗口不存在（尚未创建或已关闭），用户展开托盘菜单
- **THEN** 「打开控制台」置灰不可点，「以管理员自动登录」可用

#### Scenario: 窗口重建后恢复可用
- **WHEN** 通过 Dock 重开或托盘「以管理员自动登录」重建控制台窗口
- **THEN** 「打开控制台」恢复可用

#### Scenario: 窗口销毁后重新置灰
- **WHEN** 控制台窗口被用户关闭，或因服务重启 / 退出被应用销毁而应用继续运行
- **THEN** 「打开控制台」重新置灰

### Requirement: Dock 图标存在性

Dock 图标 SHALL 跟随控制台窗口存在性显隐（macOS activation policy）：控制台窗口存在时以 `regular` 形态运行（Dock 图标可见）；不存在时（关进托盘、`--hidden` 启动未开窗）以 `accessory` 形态运行（Dock 图标隐藏），应用与托盘继续运行。

判据与「打开控制台菜单项可用性」同源：仅控制台主窗口计入，splash、notice 等瞬态窗口不计入。MUST NOT 采用恒隐藏形态（`LSUIElement`）：窗口存在时 Dock 图标 MUST 可见。

显隐切换 MUST 在窗口创建与销毁两个时机执行；隐藏 Dock 图标 MUST NOT 影响托盘图标与已展示窗口的可见性。其他平台 MUST NOT 因本需求产生任何行为变化。

#### Scenario: 关窗进托盘后 Dock 图标消失
- **WHEN** 偏好「关闭窗口时最小化到托盘」开启，用户关闭控制台窗口
- **THEN** Dock 图标消失，应用与 `solidway` 子进程继续运行，托盘图标仍在

#### Scenario: 窗口重建后 Dock 图标出现
- **WHEN** 通过托盘「以管理员自动登录」或「Dock 重开控制台」兕底路径重建控制台窗口
- **THEN** Dock 图标恢复可见

#### Scenario: 窗口存在时 Dock 图标可见
- **WHEN** 控制台窗口存在（含最小化）
- **THEN** Dock 图标可见，点击后窗口显示并聚焦

#### Scenario: --hidden 启动不显示 Dock 图标
- **WHEN** 以 `--hidden` 启动且尚未创建控制台窗口
- **THEN** Dock 图标不显示；首次创建控制台窗口后恢复可见

#### Scenario: 瞬态窗口不改变 Dock 图标
- **WHEN** 仅 splash、notice 等瞬态窗口可见（启动中、失败提示等）
- **THEN** Dock 图标保持隐藏

### Requirement: 桌面登录令牌

`solidway` 在 desktop 模式启动时 SHALL 生成进程级桌面登录令牌，内存中只保留其 SHA-256，并在 bind 成功后将 `solidway: desktop-login url=…` 输出到 stdout。

该事件行 MUST NOT 写入 `<data_root>/solidway.log`。standalone 模式 MUST NOT 生成令牌，也 MUST NOT 输出该事件。

令牌 MUST 在进程生命周期内可重复使用，进程重启 MUST 轮换；MUST NOT 落库，MUST NOT 由任何需要认证的接口返回。

#### Scenario: 令牌经事件交付
- **WHEN** 服务以 desktop 模式启动并成功 bind
- **THEN** stdout 出现包含完整 `url` 的 `desktop-login` 行

#### Scenario: 独立模式无令牌
- **WHEN** 服务以 standalone 模式启动
- **THEN** 不生成令牌，也不输出 `desktop-login` 事件

#### Scenario: 令牌不落日志
- **WHEN** 子进程输出 `solidway: desktop-login …`
- **THEN** 该行不写入 `solidway.log`，只保留在壳内存

#### Scenario: 进程内可复用
- **WHEN** 同一进程内用同一令牌换取会话两次
- **THEN** 两次都成功；进程重启后旧令牌失效

### Requirement: 桌面自动登录导航

应用 SHALL 优先打开桌面登录地址；未收到 `desktop-login` 事件时才回落到普通控制台地址。

应用 MUST 使用事件给出的 `url` 字段导航，MUST NOT 自行拼接令牌。

桌面登录地址 MUST 使用本机回环基址：通配绑定（`0.0.0.0` / `[::]`）MUST 归一化为 `127.0.0.1` / `[::1]`；MUST NOT 使用公网地址或 TLS 域名，因此 desktop 模式同时对外提供服务时，管理员令牌也不会经过对外的入口。

桌面自动登录 MUST 固定签发 `admin` 会话。admin 尚未设置口令时 MUST NOT 阻止自动登录：桌面会话的授权依据是进程令牌，口令是可选设置项（见 `admin-console` 的「设置界面」）。

#### Scenario: 启动后免登录进入控制台
- **WHEN** 应用启动且健康就绪
- **THEN** 控制台直接以 admin 会话打开，不出现登录页

#### Scenario: 未设置口令同样进入控制台
- **WHEN** admin 尚未设置口令
- **THEN** 仍然自动进入控制台，`/profile` 提供「设置口令」

#### Scenario: 首次引导优先
- **WHEN** 应用以独立（standalone）模式运行且 admin 引导未完成
- **THEN** 打开引导页；桌面自动登录接口不可用，桌面登录地址不参与导航

#### Scenario: 改端口或清空 webview 数据后
- **WHEN** 端口变化或 webview cookie 被清空后重启应用
- **THEN** 仍然自动进入控制台，不需要输入口令

### Requirement: 托盘管理员自动登录

托盘 SHALL 提供「以管理员自动登录」菜单项：已有控制台窗口时 MUST 导航到桌面登录地址并聚焦；无窗口时 MUST 复用打开流程。

尚未收到桌面登录事件时 MUST 展示可读错误，MUST NOT 静默失败。

#### Scenario: 托盘回到管理员
- **WHEN** 用户在登录页点击托盘「以管理员自动登录」
- **THEN** 控制台重新以 admin 会话进入

#### Scenario: 服务未就绪
- **WHEN** 服务尚未输出桌面登录事件时点击该菜单项
- **THEN** 展示可读错误提示

### Requirement: 引导完成后的地址清理

引导令牌被消费后，服务 MUST 输出 `solidway: bootstrap-consumed`；桌面壳收到后 MUST 清空引导页地址。

此后托盘「打开控制台」MUST 使用桌面登录地址，MUST NOT 再导航到 `/bootstrap#<旧令牌>`。

#### Scenario: 不再进入死链
- **WHEN** 用户完成口令设置后再从托盘打开控制台
- **THEN** 直接进入控制台，不出现引导页

### Requirement: 退出登录不自动重登

「退出登录」MUST 保持可用（用于切换用户）。显式退出后，应用 MUST NOT 自动重新登录，直到用户点击托盘入口或下次启动。

#### Scenario: 退出后停在登录页
- **WHEN** 用户在控制台点击「退出登录」
- **THEN** 停在登录页，不会被自动登录拉回

### Requirement: 桌面会话自动续期

桌面模式的控制台 SHALL 在会话失效（`401`）且本地存有桌面令牌时自动重新换取一次会话；成功后 MUST 以新会话重新初始化界面。

重新换取失败时 MUST 清除本地令牌并回到登录页。本地令牌 MUST 只保存在当前 webview 会话内（`sessionStorage`），MUST NOT 持久化到磁盘。

#### Scenario: 会话过期自动续
- **WHEN** 控制台因会话过期收到 401，且本地仍保留桌面令牌
- **THEN** 自动重新登录为 admin 并继续使用，不出现登录页

#### Scenario: 令牌失效
- **WHEN** 令牌无效或服务已轮换令牌
- **THEN** 清除本地令牌并展示登录页

### Requirement: 首次启动与口令引导

`desktop` 模式下 `solidway` MUST NOT 签发引导令牌；应用 SHALL 在健康就绪后直接打开桌面登录地址（免登录进入控制台），MUST NOT 展示「设置口令」页面，MUST NOT 要求用户设置口令才能使用。

`standalone` 模式下行为不变：`solidway` 在 `data_root` 首次初始化（或 admin 引导未完成）时向标准输出打印一次性引导令牌，由终端用户访问 `/bootstrap#<token>` 设置口令。

应用 MUST NOT 将任何含令牌的事件行（`solidway: bootstrap …` / `solidway: desktop-login …`）写入 `<data_root>/solidway.log`，这些令牌只保留在壳内存中。

当本次启动收到 `bootstrap` 事件时，应用 MUST 使用事件里的 `url` 字段打开窗口，MUST NOT 自行拼接路径或令牌。

#### Scenario: 桌面首启免登录进入控制台
- **WHEN** 应用以桌面模式首次启动且健康就绪
- **THEN** 控制台窗口直接以 admin 会话打开，不出现设置口令页

#### Scenario: 桌面模式无引导事件
- **WHEN** 子进程 stdout 中没有 `bootstrap` 事件（desktop 模式恒定如此）
- **THEN** 应用使用 `desktop-login` 事件给出的地址导航，不回落到设置口令页

#### Scenario: 独立模式首启进入设置口令
- **WHEN** `standalone` 模式启动并收到 `bootstrap` 事件
- **THEN** 终端输出的引导地址可直接打开 `/bootstrap#<token>` 设置口令

#### Scenario: 非首启直接进入控制台
- **WHEN** 未收到 `bootstrap` 事件，且健康探测成功
- **THEN** 应用直接打开控制台，不展示任何凭证

#### Scenario: 令牌不落日志
- **WHEN** 子进程 stdout 中出现 `solidway: bootstrap` 或 `solidway: desktop-login` 行
- **THEN** 该行不写入 `solidway.log`

### Requirement: 桌面壳偏好持久化

桌面壳 SHALL 把两个生命周期偏好持久化在 `<data_root>/desktop.toml`：`autostart`（开机自启动）与 `close_window_to_tray`（关闭窗口时最小化到托盘）。

桌面壳 MUST 是该文件的唯一写者；`solidway` 服务 MUST NOT 写该文件，MAY 只读它以向控制台展示当前值。

桌面壳每次启动 MUST 把生效值**物化**写入该文件（文件缺失或无法解析时以默认值重写），使「文件内容」恒等于「壳实际执行的行为」。写入 MUST 原子提交（临时文件 + 原子替换）。

默认值：`close_window_to_tray` MUST 为 `true`；`autostart` 在发布构建 MUST 为 `true`，在 debug 构建 MUST 为 `false`。文件或字段缺失时 MUST 使用默认值；文件无法解析时 MUST 使用默认值并继续启动，MUST NOT 阻止应用启动。

#### Scenario: 首次运行物化偏好
- **WHEN** `<data_root>/desktop.toml` 不存在
- **THEN** 壳以默认值写入该文件，并按默认值执行开机自启动与关窗策略

#### Scenario: debug 构建默认不自启
- **WHEN** debug 构建首次启动且无偏好文件
- **THEN** 不注册系统登录项，文件中 `autostart = false`

#### Scenario: 偏好文件损坏
- **WHEN** `desktop.toml` 内容无法解析
- **THEN** 壳以默认值启动、重写该文件并记录一条壳日志，不因此拒绝启动

#### Scenario: 偏好跨重启保留
- **WHEN** 用户关闭「开机自启动」后重启应用
- **THEN** 勾选仍为关闭，系统登录项保持不存在

### Requirement: 开机自启动（默认开启，可关闭）

桌面壳 SHALL 支持开机自启动，默认值见「桌面壳偏好持久化」。

开启时 MUST 向当前用户登录启动项注册本应用：Linux 写入用户级 autostart 目录的 `.desktop`（`~/.config/autostart`），macOS 写入 `~/Library/LaunchAgents` 的 plist，Windows 写入当前用户 `Run` 注册表项。注册命令 MUST 携带 `--hidden`，且 MUST 只包含可执行文件路径与该参数，MUST NOT 包含 `data_root`、引导令牌或任何密钥材料。

关闭时 MUST 移除该登录项。

注册或注销失败时 MUST 保持偏好文件不变，并通过「桌面偏好回执」报告失败原因；MUST NOT 静默失败。

桌面壳每次启动 SHALL 依据偏好对系统登录项执行一次对账：偏好开启而登录项缺失时 MUST 重建，偏好关闭而登录项仍存在时 MUST 移除。对账失败 MUST 记录到壳日志并写一条失败回执，MUST NOT 阻塞启动。

#### Scenario: 默认注册
- **WHEN** 发布构建首次启动
- **THEN** 系统登录项存在且命令包含 `--hidden`

#### Scenario: 关闭开机自启动
- **WHEN** 用户在控制台或托盘关闭「开机自启动」
- **THEN** 登录项被移除，偏好文件写入 `autostart = false`

#### Scenario: 注册失败
- **WHEN** 系统拒绝写入登录项（权限或策略限制）
- **THEN** 偏好文件不变，回执 `ok = false` 且含可读原因

#### Scenario: 登录项被手工删除
- **WHEN** 偏好为开启但系统登录项缺失，应用下次启动
- **THEN** 应用重新注册登录项

### Requirement: 静默启动

应用 SHALL 接受 `--hidden` 参数。以该参数启动时，应用 MUST NOT 主动展示控制台窗口或 splash 窗口，运行状态 SHALL 只体现在托盘。

静默启动进入失败态（启动失败、握手超时、端口冲突、子进程退出）时 MUST 展示可读错误页，MUST NOT 静默失败。

静默启动且存在未消费的引导令牌时，应用 MUST NOT 自动打开引导页；托盘「打开控制台」SHALL 仍指向引导页。

#### Scenario: 登录自启
- **WHEN** 系统登录项以 `--hidden` 启动应用且服务就绪
- **THEN** 不出现任何窗口，托盘显示「运行中」

#### Scenario: 静默启动失败
- **WHEN** 以 `--hidden` 启动后服务启动失败或端口冲突
- **THEN** 展示与手动启动一致的可读错误页

#### Scenario: 首次引导期静默启动
- **WHEN** 以 `--hidden` 启动且存在未消费的引导令牌
- **THEN** 不自动打开窗口；用户点击托盘「打开控制台」时进入引导页

### Requirement: 单实例

同一应用 MUST 只运行一个桌面壳进程与一个 `solidway` 子进程。再次启动应用 SHALL 唤醒已有实例后立即退出，MUST NOT 派生第二个子进程。

转发参数不含 `--hidden` 时，已有实例 SHALL 显示并聚焦控制台（引导期显示引导页）；转发参数含 `--hidden` 时 MUST NOT 抢占焦点。

#### Scenario: 自启后再次手动启动
- **WHEN** 开机自启实例已在托盘运行，用户再次启动应用
- **THEN** 已有实例显示控制台，第二个进程退出，不新增子进程且不出现端口冲突

#### Scenario: 静默二次启动
- **WHEN** 第二个实例以 `--hidden` 启动
- **THEN** 已有实例不弹窗、不抢焦点，第二个进程退出

### Requirement: 桌面偏好回执

`solidway` 在 desktop 模式下收到控制台的偏好修改请求后，MUST 通过 stdout 事件 `solidway: desktop-apply id=<8hex> autostart=<0|1> close_to_tray=<0|1>` 通知桌面壳。

桌面壳 MUST 解析该事件并按其意图执行：应用开机自启动状态、更新关窗策略、把结果写回偏好文件。

桌面壳 MUST 在每次执行后原子写 `<data_root>/desktop.status.json`，字段为 `apply_id`、`ok`、`error`、`applied`、`at`。`applied` MUST 反映实际生效的值；失败时 `error` MUST 为可读原因，且偏好文件 MUST 保持操作前的值。

对同一 `apply_id` 的重复事件 MUST 幂等；无法解析的事件行 MUST 被忽略且 MUST NOT 影响其他事件的处理。

#### Scenario: 修改成功
- **WHEN** 壳收到 `desktop-apply id=3f2a1c9d autostart=1 close_to_tray=0` 且注册成功
- **THEN** 偏好文件更新为对应值，回执 `apply_id = 3f2a1c9d, ok = true, applied` 与文件一致

#### Scenario: 修改失败
- **WHEN** 壳收到事件但系统拒绝注册登录项
- **THEN** 偏好文件保持旧值，回执 `ok = false` 且含原因，`applied.autostart` 仍为旧值

#### Scenario: 重复或未知事件
- **WHEN** 壳重复收到同一 `apply_id` 的事件，或收到无法解析的事件行
- **THEN** 重复事件得到相同结果，无法解析的行被忽略且不影响后续事件
