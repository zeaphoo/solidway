# platform-support Specification

## Purpose
TBD - created by archiving change platform-parity. Update Purpose after archive.

## Requirements

### Requirement: 三平台支持

solidway 的全部功能 MUST 支持 Linux、Windows、macOS；MUST NOT 依赖仅单一平台可用的构建工具链或运行时组件（如 bindgen / libclang）。

#### Scenario: 无额外构建依赖
- **WHEN** 在干净的 Linux / macOS / Windows 上构建
- **THEN** 只需要 Rust 工具链，不需要 libclang 或其他 C 工具链

### Requirement: 因子进程托管方消失而退出

以 `--mode desktop` 启动时，若托管方的 stdin 管道被关闭（托管方崩溃或被强杀），进程 MUST 立即退出，MUST NOT 继续占用端口。

`standalone` 模式 MUST NOT 启用该行为。

#### Scenario: 托管方崩溃
- **WHEN** 桌面应用被强杀
- **THEN** `solidway` 在收到 stdin EOF 后立即退出，端口被释放

#### Scenario: 独立模式不受影响
- **WHEN** 以 `--mode standalone` 且 stdin 为 `/dev/null` 启动
- **THEN** 进程不因 stdin EOF 退出

### Requirement: 端口占用者诊断

`bind-failed` 事件 SHOULD 包含占用该端口的进程标识（`holder_pid` / `holder_comm`）。

该诊断 MUST 是尽力而为：任何失败都 MUST NOT 改变退出行为，也 MUST NOT 输出进程的完整路径或命令行参数。

#### Scenario: 能识别占用者
- **WHEN** 端口被同用户进程占用且权限允许
- **THEN** 事件含 `holder_pid` 与 `holder_comm`

#### Scenario: 无法识别
- **WHEN** 权限不足或实现不支持
- **THEN** 事件只含 `addr` 与 `kind`，进程仍以非零码退出

### Requirement: 主密钥文件权限

Unix 上 `<data_root>/master.key` MUST 以 `0600` 创建。Windows 上 MUST 依赖 `data_root` 目录 ACL，并在文档中要求该目录不得位于共享位置。

#### Scenario: Unix 权限
- **WHEN** 首次生成主密钥
- **THEN** 文件权限为 `0600`

### Requirement: TLS 监听与 HTTP 跳转

`tls.enabled=false` 时，服务 MUST 保持现有内部 HTTP 监听，继续支持由反向代理终止 TLS。

`tls.enabled=true` 时，服务 MUST 在 `https_port` 提供 TLS（rustls，ALPN `h2` / `http/1.1`），并在 `http_port` 提供 ACME HTTP-01 挑战与 301/308 跳转。ACME 挑战路径 MUST 优先于跳转；端口绑定失败 MUST 返回可读错误，MUST NOT 静默降级到明文。

证书未就绪时（首次签发中或签发失败），`http_port` MUST 只提供 ACME 挑战与 503 可读提示，MUST NOT 跳转到 `https://`（目标端口尚无证书可用），MUST NOT 以明文提供应用界面。

#### Scenario: 关闭时保持 HTTP
- **WHEN** `tls.enabled=false`
- **THEN** 行为与改动前一致

#### Scenario: HTTP 跳转
- **WHEN** 访问 `http://gw.example.com/foo?a=1`
- **THEN** 跳转到 `https://gw.example.com/foo?a=1`

#### Scenario: ACME 不被跳转
- **WHEN** 请求 `/.well-known/acme-challenge/<token>`
- **THEN** 直接返回挑战内容，不跳转

#### Scenario: 证书未就绪
- **WHEN** `tls.enabled=true` 但尚无有效证书
- **THEN** HTTP 只提供 ACME 挑战与 503 提示，不跳转、不明文服务应用

#### Scenario: 端口绑定失败
- **WHEN** 80/443 被占用或权限不足
- **THEN** 返回可读错误，不静默降级到明文

### Requirement: 域名边界与内部监听

启用 TLS 时，服务 MUST 只对参与证书的域名提供服务；未知 Host MUST NOT 被跳转到任意地址。MUST NOT 在公网接口额外暴露明文应用入口；内部监听若保留，MUST 仅绑定 loopback。

standalone 与 desktop 模式 MUST 共用该逻辑；desktop 模式默认 `enabled=false`，启用前 MUST 完成与 standalone 相同的校验。

启用内置 TLS 时，服务 MUST NOT 依据 `X-Forwarded-Proto` / `Forwarded` 等代理头判断「已由外部终止 TLS」；`Secure` Cookie、HSTS 与跳转决策 MUST 只由 `tls` 设置决定。内置 TLS 与外部反代同时启用不属于推荐部署形态，文档 MUST 说明二者择一。

#### Scenario: 未知 Host
- **WHEN** 请求的 Host 不在证书域名内
- **THEN** 不跳转到该 Host

#### Scenario: 忽略代理协议头
- **WHEN** 内置 TLS 已启用且请求带 `X-Forwarded-Proto: https`
- **THEN** 该头不影响 `Secure` Cookie、HSTS 与跳转判断

#### Scenario: 无明文公网入口
- **WHEN** TLS 已启用
- **THEN** 除 80 的 ACME / 跳转外，不存在公网明文应用入口

#### Scenario: 桌面模式默认关闭
- **WHEN** 以 desktop 模式首次启动
- **THEN** `tls.enabled=false`

### Requirement: ACME 首次签发

启用 TLS 后，服务 SHALL 使用配置的 ACME directory 自动申请证书；challenge 默认 `http-01`，MAY 使用 `tls-alpn-01`。

- 首次无缓存证书时 MUST NOT 因缺少证书而崩溃；
- `http-01` 首次签发不依赖任何预置证书；
- `tls-alpn-01` MUST 使用临时自签名挑战证书（含 `id-pe-acmeIdentifier`）响应 `acme-tls/1`，MUST NOT 把挑战证书暴露给普通 ALPN；
- 证书 MUST 为覆盖全部合法域名的 SAN 证书；域名集合变化 MUST 触发重新签发。

#### Scenario: 首次 HTTP-01
- **WHEN** 没有缓存证书且 challenge=http-01
- **THEN** 80 端口完成挑战并签发证书，HTTPS 开始可用

#### Scenario: 首次 TLS-ALPN-01
- **WHEN** 没有缓存证书且 challenge=tls-alpn-01
- **THEN** 443 以临时挑战证书通过校验并签发正式证书

#### Scenario: 域名变化
- **WHEN** `access.public_origins` 增加或删除合法域名
- **THEN** 重新签发覆盖新域名集合的 SAN 证书

### Requirement: ACME 挑战端口语义

`http_port` / `https_port` MUST 被理解为**本地监听端口**；ACME 校验使用的公网端口固定为 80（`http-01`）与 443（`tls-alpn-01`），由部署侧的端口映射或反向代理决定。

当 `challenge=http-01` 且 `http_port≠80`，或 `challenge=tls-alpn-01` 且 `https_port≠443` 时：启用 MUST 仍然允许；`GET /api/tls/status` MUST 返回 `challenge_port_hint`，说明所需的公网端口映射；控制台 MUST 以警告展示。系统 MUST NOT 假设端口已映射，也 MUST NOT 为规避该约束而自动切换 challenge 类型。

端口不通导致的签发失败 MUST 走同一失败退避与状态上报，MUST NOT 崩溃、MUST NOT 降级为明文服务。

#### Scenario: 非 80 端口启用 HTTP-01
- **WHEN** `challenge=http-01` 且 `http_port=8080`
- **THEN** 启用成功，状态含 `challenge_port_hint`（需 80→8080 映射），控制台显示警告

#### Scenario: 非 443 端口启用 TLS-ALPN-01
- **WHEN** `challenge=tls-alpn-01` 且 `https_port=8443`
- **THEN** 启用成功，状态含 `challenge_port_hint`（需 443→8443 映射）

#### Scenario: 自动切换被禁止
- **WHEN** `challenge=http-01` 且 `http_port≠80`
- **THEN** 系统不改为 `tls-alpn-01`，只在状态中提示

#### Scenario: 端口不通
- **WHEN** 80 未映射到 `http_port`，`http-01` 校验不可达
- **THEN** 签发失败并进入退避，状态可读，服务不崩溃、不明文降级

### Requirement: 证书存储与回退

account key、证书与私钥 MUST 存放于 `data_root/tls/`；Unix 权限 MUST 为 `0600`；写入 MUST 原子化；私钥 MUST NOT 写入日志。

证书缓存文件名 MUST 由当前域名集合的稳定摘要派生，MUST NOT 直接用域名拼接。域名集合变化后 MUST 保留最近一份未过期证书作为回退，其余旧证书文件 MUST 清理。启动时 MUST 优先使用与当前域名集合完全匹配的未过期证书；无匹配时 MAY 用最近一份未过期证书继续服务，并同时发起新签发。

#### Scenario: 权限与原子写
- **WHEN** 写入 account key 或证书
- **THEN** 文件权限为 `0600`，且以 tmp + rename 原子落盘

#### Scenario: 域名集合变化后的缓存
- **WHEN** `access.public_origins` 删除一个域名并触发重新签发
- **THEN** 新证书按新摘要命名，旧证书保留一份作回退、其余清理

#### Scenario: 启动回退
- **WHEN** 缓存中没有与当前域名集合完全匹配的证书，但存在一份未过期旧证书
- **THEN** 先用旧证书提供服务，同时发起新签发

### Requirement: 证书续期与退避

到期前 30 天 MUST 开始续期。失败 MUST 保留旧证书并退避重试：重试间隔 MUST 指数增长，起点 MUST NOT 超过 5 分钟、上限 MUST NOT 超过 1 小时，并 MUST 带抖动。同一证书在 24 小时内累计失败达到 5 次后 MUST 停止自动重试并进入 `paused` 状态，直到人工重试或进程重启；MUST NOT 无上限地高频重试。成功后 MUST 热替换证书，MUST NOT 要求重启；MUST 支持 production / staging directory。

#### Scenario: 续期失败
- **WHEN** 续期请求失败但旧证书仍未过期
- **THEN** 继续使用旧证书，记录错误并按退避重试

#### Scenario: 退避上限
- **WHEN** 续期连续失败
- **THEN** 重试间隔按指数增长且不超过 1 小时，带抖动

#### Scenario: 连续失败后暂停
- **WHEN** 同一证书 24 小时内累计失败 5 次
- **THEN** 停止自动重试，状态为 `paused`，等待人工重试或重启

#### Scenario: 热替换
- **WHEN** 续期成功
- **THEN** 新证书立即生效，无需重启

#### Scenario: staging
- **WHEN** directory=staging
- **THEN** 使用 ACME staging 端点，控制台显示 staging 警告

### Requirement: TLS 安全默认与单实例约束

启用 TLS 后，会话 Cookie MUST 设置 `Secure`。

HSTS MUST 仅在 `tls.hsts=true` 时发送；默认 MUST 为关闭。系统 MUST NOT 提供 HSTS preload 开关。

同一数据目录 / 域名集合 MUST 只由一个进程执行 ACME；多实例并发签发会竞争账户、订单与证书文件，不在支持范围内。

#### Scenario: Secure Cookie
- **WHEN** `tls.enabled=true`
- **THEN** 会话 Cookie 带 `Secure`

#### Scenario: 单实例
- **WHEN** 两个实例同时使用同一数据目录执行 ACME
- **THEN** 不属于支持场景，文档必须明确该约束

### Requirement: 多地址监听

服务 SHALL 在 `network.bind_addrs` 的**每个地址**上监听同一端口：

- 任一地址绑定失败 MUST 视为启动失败（输出 `bind-failed` 事件并非零退出），MUST NOT 静默跳过；
- 每个成功绑定的地址 MUST 各输出一行 `solidway: listening addr=…`；
- 启用内置 TLS 时，内部监听 MUST 收缩为回环地址；`https_port` 与 `http_port` MUST 在配置地址中的**非回环地址**上监听，若配置里只有回环地址则回退为按配置监听；
- `--listen` 命令行覆盖 MUST 只作用于主监听，且为单地址。

#### Scenario: 两个地址同时服务
- **WHEN** `bind_addrs = ["127.0.0.1", "192.168.1.10"]`
- **THEN** 两个地址的同一端口都响应 `/health`

#### Scenario: 某个地址绑定失败
- **WHEN** 其中一个地址已被占用
- **THEN** 输出 `bind-failed addr=…` 并非零退出，不启动半套服务

#### Scenario: TLS 跳过回环
- **WHEN** 启用内置 TLS 且地址列表含回环与局域网地址
- **THEN** `https_port` / `http_port` 只在局域网地址上监听

#### Scenario: 纯回环配置
- **WHEN** 启用内置 TLS 且地址列表只有 `127.0.0.1`
- **THEN** `https_port` / `http_port` 在 `127.0.0.1` 上监听（开发场景可用）

### Requirement: 优雅关闭

进程收到 `SIGTERM` / `SIGINT`，或桌面模式请求重启时，MUST 执行一次收尾后再退出：

1. 停止接受新连接；
2. 等待在途请求完成（有上限，建议 2s）；
3. 落盘用量缓冲一次；
4. 导出并写入运行期状态快照一次；
5. 退出。

收尾 MUST 受总时长上限约束（建议 5s），超时 MUST 强制退出，MUST NOT 无限等待。收尾过程中的任何失败 MUST 只记录告警，MUST NOT 改变退出码（避免托管方把"收尾失败"当成服务异常而重启循环）。

桌面模式的两条退出路径（stdin EOF 看门狗、`/api/app/restart` 触发的退出）MUST 走同一收尾流程，并同样受超时约束。

#### Scenario: SIGTERM 收尾
- **WHEN** 进程收到 `SIGTERM` 且内存中有未落盘的用量增量
- **THEN** 这些增量被落盘后才退出

#### Scenario: 收尾超时
- **WHEN** 落盘或快照写入超过总预算
- **THEN** 进程强制退出，退出码仍为 0

#### Scenario: 收尾失败不改变退出码
- **WHEN** 落盘失败（如 DB 不可用）
- **THEN** 记录告警并以 0 退出，MUST NOT 阻塞或循环重试

#### Scenario: 端口释放
- **WHEN** 收尾完成或超时强退
- **THEN** 监听端口被释放，可立即重启
