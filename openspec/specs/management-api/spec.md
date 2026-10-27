# management-api Specification

## Purpose
TBD - created by archiving change add-management-api. Update Purpose after archive.

## Requirements

### Requirement: API 前缀与响应契约

管理面所有 HTTP 接口 MUST 挂在 `/api` 前缀下；请求体与响应体 MUST 为 JSON（`GET /api/health` 除外）。

失败响应 MUST 使用统一结构 `{ "error": { "code": "<机器可读码>", "message": "<人类可读说明>" } }`，并返回对应的 HTTP 状态码。系统 MUST NOT 在响应中泄露内部错误细节或堆栈。

#### Scenario: 未认证访问受保护接口
- **WHEN** 客户端未携带有效会话 Cookie 请求 `/api/routes`
- **THEN** 服务返回 `401`，响应体为统一错误结构，`code` 为 `unauthorized`

#### Scenario: 请求体校验失败
- **WHEN** 客户端提交缺少必填字段的请求体
- **THEN** 服务返回 `422`，响应体为统一错误结构，`code` 为 `invalid_request`

### Requirement: 账号登录与会话

系统 SHALL 提供基于账号口令的登录。口令 MUST 以 Argon2id 哈希存储，SHALL NOT 以任何形式明文存储或返回。

登录成功后，系统 MUST 签发随机会话令牌，通过 HttpOnly Cookie 下发，并在服务端 `sessions` 表记录其哈希与过期时间。

#### Scenario: 登录成功
- **WHEN** 客户端以正确用户名与口令请求 `POST /api/auth/login`
- **THEN** 服务返回 `200`，下发 HttpOnly 会话 Cookie，并返回当前用户信息（不含口令）

#### Scenario: 凭据错误
- **WHEN** 客户端以错误用户名或口令请求 `POST /api/auth/login`
- **THEN** 服务返回 `401`，`code` 为 `invalid_credentials`，且不区分「用户名不存在」与「口令错误」

#### Scenario: 会话过期
- **WHEN** 使用已过期会话的 Cookie 访问受保护接口
- **THEN** 服务返回 `401`，`code` 为 `unauthorized`

#### Scenario: 退出登录
- **WHEN** 客户端请求 `POST /api/auth/logout`
- **THEN** 服务删除对应会话记录并清除 Cookie

#### Scenario: 读取当前登录用户
- **WHEN** 客户端携带有效会话请求 `GET /api/auth/me`
- **THEN** 服务返回当前用户信息（不含口令）

### Requirement: Admin 引导创建

`solidway` 启动时 SHALL 检查是否存在 `admin` 角色的用户。

若不存在，MUST 创建用户名 `admin` 的账号，其口令哈希 MUST 处于**锁定态**（不可用于登录）。

`standalone` 模式下 MUST 向标准输出打印一次性**引导令牌**（bootstrap token）；`desktop` 模式下 MUST NOT 签发或打印引导令牌 —— 桌面身份由进程令牌承担，admin 允许以「未设置口令」状态存在，口令在控制台内可选设置（见「个人资料与口令」）。

引导令牌 MUST 为至少 32 字节的随机值；MUST NOT 写入任何持久化存储（数据库、日志、文件）。

令牌 SHALL 只存在于签发进程的内存中，且内存中 MUST 只保留其 SHA-256。需要跨重启保留的只有 `users.password_hash` 的锁定态。

`standalone` 模式下只要 `admin` 仍处于锁定态，每次启动 MUST 重新签发令牌并作废此前令牌。

#### Scenario: 首次启动无 admin
- **WHEN** 数据库中不存在 admin 用户，服务以 `standalone` 模式启动
- **THEN** 系统创建锁定态的 `admin` 用户，并向标准输出打印一次性引导令牌

#### Scenario: 桌面模式首次启动
- **WHEN** 数据库中不存在 admin 用户，服务以 `desktop` 模式启动
- **THEN** 系统创建锁定态的 `admin` 用户，MUST NOT 打印任何引导令牌

#### Scenario: 桌面模式不签发引导
- **WHEN** 服务以 `desktop` 模式启动，且 admin 口令哈希仍为锁定态
- **THEN** `GET /api/auth/bootstrap` 返回 `pending=false`，且启动过程中不打印引导令牌

#### Scenario: 引导未完成时重启
- **WHEN** `standalone` 模式下数据库中 `admin` 存在且口令哈希仍为锁定态
- **THEN** 系统签发新令牌并打印，上一进程的旧令牌自然失效（不依赖任何持久化状态）

#### Scenario: 已存在 admin
- **WHEN** 数据库中 `admin` 存在且口令哈希合法
- **THEN** 系统不创建用户、不签发令牌、不打印任何凭证（两种运行模式一致）

#### Scenario: 锁定态无法口令登录
- **WHEN** 客户端以 `admin` + 任意口令请求 `POST /api/auth/login`（admin 尚未设置口令）
- **THEN** 服务返回 `401`，`code` 为 `invalid_credentials`（`standalone` 与 `desktop` 一致）

### Requirement: 路由管理

系统 SHALL 提供「路由（route）」的增删改查；路由是面向消费方、按名唯一寻址的 LLM 服务单位，且 MUST 为用户私有资源。

每条路由 MUST 携带 `owner_id`。新建路由的 `owner_id` MUST 为当前登录用户；客户端 MUST NOT 通过创建或更新接口指定其他 owner。活跃路由名称 MUST 在同一 owner 内唯一，不同 owner MAY 使用相同名称。归档路由 MUST NOT 占用名称，允许存在多个同名归档路由。

历史路由（`owner_id IS NULL`）MUST 在 `ensure_admin` 之后统一归属「最早的 admin」（`created_at ASC, id ASC`），回填 MUST 幂等。

`GET /api/routes` MUST 默认只返回当前用户拥有的未归档路由；`GET/PATCH/DELETE /api/routes/{id}` MUST 只允许 owner 访问，其他用户（包括 admin）MUST 返回 `404`。`admin` MUST NOT 拥有跨用户的路由管理权限，但 MAY 通过专用转移接口执行 offboarding。

删除路由 MUST 为不可恢复的归档语义：保留 Route 行、用量与故障历史，释放名称，并删除 Provider / API Key 活行、销毁其密钥材料。用户仍拥有未归档路由时，归档该用户 MUST 返回 `409`；系统 MUST NOT 级联删除其活跃路由。

#### Scenario: 创建路由
- **WHEN** 已认证用户请求 `POST /api/routes`，携带唯一 `name`
- **THEN** 服务创建路由，`owner_id` 为当前用户，并返回 `201` 与路由对象

#### Scenario: 名称冲突
- **WHEN** 用户以自己已有的 `name` 创建或重命名路由
- **THEN** 服务返回 `409`，`code` 为 `conflict`

#### Scenario: 不同 owner 同名
- **WHEN** 用户 A 与用户 B 分别创建名为 `prod` 的路由
- **THEN** 两条路由均创建成功，且彼此不可见

#### Scenario: 列举与查询
- **WHEN** 已认证用户请求 `GET /api/routes` 或 `GET /api/routes/{id}`
- **THEN** 服务只返回该用户拥有的路由；不存在或不属于该用户时返回 `404`，`code` 为 `not_found`

#### Scenario: admin 不能跨用户访问
- **WHEN** admin 请求普通用户拥有的路由
- **THEN** 服务返回 `404`，且不返回该路由内容

#### Scenario: 删除路由
- **WHEN** 路由 owner 请求 `DELETE /api/routes/{id}`
- **THEN** 服务归档该路由并返回 `204`；名称释放，Provider / API Key 删除，Route 行与历史保留

#### Scenario: 历史路由归属
- **WHEN** 系统从没有 `owner_id` 的旧版本升级
- **THEN** 所有历史路由归属最早的 admin，并在该 admin 的路由列表中可见

#### Scenario: 删除仍拥有路由的用户
- **WHEN** admin 删除（归档）一个仍拥有未归档路由的用户
- **THEN** 服务返回 `409`

### Requirement: provider 凭据管理

系统 SHALL 支持向指定路由挂载第三方 provider 凭据（provider key），并支持查询、更新与删除。

凭据 MUST 关联一个 provider（内置预设 key 或自定义值），并包含 `base_url`、协议格式与 api_key。

#### Scenario: 新增凭据
- **WHEN** 已认证用户请求 `POST /api/routes/{id}/provider-keys`，携带 `provider` 与 `api_key`
- **THEN** 服务创建凭据；未显式提供的 `base_url` 与协议格式 MUST 由对应 provider 预设补全

#### Scenario: 读取凭据不回传明文
- **WHEN** 已认证用户请求 `GET /api/routes/{id}/provider-keys` 或 `GET /api/provider-keys/{id}`
- **THEN** 响应 MUST NOT 包含 api_key 明文，只返回掩码（如末 4 位）

#### Scenario: 更新凭据
- **WHEN** 已认证用户请求 `PATCH /api/provider-keys/{id}`
- **THEN** 服务更新允许修改的字段；若携带新 api_key，则重新加密存储

#### Scenario: 删除凭据
- **WHEN** 已认证用户请求 `DELETE /api/provider-keys/{id}`
- **THEN** 服务删除该凭据，返回 `204`

### Requirement: provider 预设目录

系统 SHALL 内置常见第三方 provider 的预设（协议族、默认 base_url、鉴权方式），并通过 `GET /api/providers` 暴露，供前端选择。

预设数据 MUST 由构建期生成的静态目录（`providers.json`）提供并编入二进制；运行时 MUST NOT 依赖外部网络获取该目录。

#### Scenario: 列举预设
- **WHEN** 已认证用户请求 `GET /api/providers`
- **THEN** 服务返回内置 provider 预设列表，条带 `featured` 标记

### Requirement: 凭据加密存储

provider 的 api_key MUST 以 AES-256-GCM 加密后落库；数据密钥 MUST 通过 HKDF-SHA256 从主密钥域分离派生；密文 MUST 携带密钥 id 与版本前缀，格式为 `sw:v1:<kid>:<iv>:<ct>:<tag>`；AAD MUST 绑定该凭据的记录标识。

系统 MUST NOT 在任何日志或接口响应中输出 api_key 明文或主密钥。

#### Scenario: 主密钥缺失
- **WHEN** 数据库中已有密文但主密钥不可用
- **THEN** 服务 MUST 以明确错误失败（fail fast），SHALL NOT 静默跳过或降级为明文

#### Scenario: 密文被移植
- **WHEN** 一条凭据的密文被复制到另一条记录
- **THEN** 解密 MUST 因 AAD 校验失败而报错

#### Scenario: 密钥轮换
- **WHEN** 主密钥配置为包含新旧多个值（第一个为当前）
- **THEN** 新写入的密文 MUST 使用当前密钥的 `kid`；旧密文 MUST 仍能按其 `kid` 正确解密

### Requirement: 启动与运行时配置

`solidway` SHALL 接受且仅接受一个命令行参数 `data_root`，并在该目录下读取或创建运行时数据：`solidway.db`、`config.toml`、`master.key`。

配置 SHALL 从 `<data_root>/config.toml` 读取（监听地址、端口、会话 TTL、主密钥路径等）。主密钥 MAY 通过环境变量 `SOLIDWAY_MASTER_KEY` 覆盖。

`solidway` SHALL 加载并启动 `solidway-api`，由其提供 `/api` 服务。

#### Scenario: 目录不存在
- **WHEN** 传入的 `data_root` 不存在
- **THEN** 系统创建该目录并完成初始化

#### Scenario: 加载并启动
- **WHEN** `solidway` 以合法 `data_root` 启动
- **THEN** 系统连接 `<data_root>/solidway.db`、执行迁移、确保 admin、启动 `/api` 服务并监听配置的地址

#### Scenario: 空参数
- **WHEN** 未提供 `data_root` 参数启动
- **THEN** 系统打印用法并以非零码退出

### Requirement: provider 目录的内容与来源

目录 SHALL 覆盖常见第三方 provider，每个条目 MUST 至少包含 `key`、`name`、`base_url_template`、`auth_scheme`、`kind`，并 MAY 含 `doc`、`featured`、`summary`、`note`。

目录的机械来源 MUST 为 models.dev 的 provider 目录；`auth_scheme`、模板、参数、入站格式与说明元数据 MUST 由受版本控制的映射 / 覆盖表提供，SHALL NOT 依赖运行时推断。

featured provider 的 `default_base_url` MUST 可用；对 models.dev 未提供 `api` 字段的 featured provider，MUST 由覆盖表补全。

支持原生入站家族的 provider，其 `default_base_url` MUST 是该上游的 **API 根**（含版本段），网关只在根后面拼接家族内端点相对路径：

- `anthropic` MUST 为 `https://api.anthropic.com/v1`；
- `google-openai` MUST 为 `https://generativelanguage.googleapis.com/v1beta/openai`，`auth_scheme = bearer`，默认格式 `chat_completions`。

上述值 MUST 由覆盖表固定，MUST NOT 采用上游目录的 `api` 字段。

目录 MUST NOT 提供 `google`、`google-vertex`、`google-vertex-global`、`amazon-bedrock`、`gitlab` 等无法用三种入站格式直连的预设；Google 模型通过 `google-openai` 接入，Bedrock Claude 通过 `bedrock-anthropic` 接入。

#### Scenario: 条目含协议族与鉴权
- **WHEN** 客户端读取目录条目
- **THEN** 每个条目都带有 `base_url_template`、`auth_scheme` 与 `kind`

#### Scenario: featured 标记
- **WHEN** 前端读取 `GET /api/providers`
- **THEN** 条目带 `featured` 标记，供前端优先展示

#### Scenario: Anthropic 预设根
- **WHEN** 客户端读取 `anthropic` 预设
- **THEN** `default_base_url` 为 `https://api.anthropic.com/v1`，`auth_scheme` 为 `x-api-key`，`kind` 为 `api`

#### Scenario: Google OpenAI 兼容预设
- **WHEN** 客户端读取 `google-openai` 预设
- **THEN** `default_base_url` 为 `https://generativelanguage.googleapis.com/v1beta/openai`，`auth_scheme` 为 `bearer`，`api_formats` 为 `["chat_completions"]`

#### Scenario: Google 原生预设已移除
- **WHEN** 客户端读取目录
- **THEN** `google`、`google-vertex`、`google-vertex-global`、`amazon-bedrock`、`gitlab` 条目均不存在；Google 模型通过 `google-openai` 接入

### Requirement: 目录可重新生成

项目 MUST 提供生成脚本，从上游目录重新生成 `providers.json`；重新生成 MUST 保持运行时不变量（产物入库、编入二进制）。

生成脚本 MUST 按 npm 包 / 覆盖表为每个条目推导三种入站格式之一；无法推导或只能使用原生协议的条目 MUST 被跳过。脚本 MUST 为每个保留条目写入 `kind`，并为 token plan / 特殊鉴权条目写入可选 `summary` / `note` key。

#### Scenario: 重新生成
- **WHEN** 维护者运行生成脚本
- **THEN** `crates/solidway_api/src/provider/providers.json` 更新，且无需改动 Rust 代码

### Requirement: 自定义 provider 仍受支持

系统 SHALL 允许录入不在内置目录中的自定义 provider；未命中目录时不带路径模板，按 `{base_url}{家族后缀}` 转发。

#### Scenario: 目录未命中
- **WHEN** 用户以目录中不存在的 `provider` 创建凭据
- **THEN** 系统接受该 provider，采用默认协议，`base_url` 取用户输入

#### Scenario: 目录命中
- **WHEN** 用户以目录中的 provider 创建凭据且未提供 `base_url`
- **THEN** 系统用目录条目的模板展开 `base_url`

### Requirement: API Key 管理

系统 SHALL 支持为路由签发客户端调用网关所用的 **API Key**。每个 API Key MUST 绑定唯一一个路由；一个路由 MAY 拥有多个 API Key。

API Key 明文 MUST 由服务端生成（形如 `sw-<随机>`），且 MUST 仅在创建响应中返回一次。数据库 MUST 只保存其哈希与用于展示的前缀 / 掩码，SHALL NOT 保存明文。

API Key MUST 可改名、停用与删除。系统 MUST NOT 在列表 / 查询响应中返回明文。

端点：

- `GET /api/api-keys` — 列出全部（含所属路由名）
- `GET /api/routes/{id}/api-keys` — 列出某路由下的 key
- `POST /api/routes/{id}/api-keys` — 创建，响应含一次性明文
- `PATCH /api/api-keys/{id}` — 改名 / 启停
- `DELETE /api/api-keys/{id}` — 删除

#### Scenario: 创建返回一次性明文
- **WHEN** 已认证用户请求 `POST /api/routes/{id}/api-keys`
- **THEN** 服务创建 key，响应 `201` 并**仅此一次**返回明文，同时返回前缀与掩码

#### Scenario: 列表只回掩码
- **WHEN** 已认证用户请求 `GET /api/api-keys` 或 `GET /api/routes/{id}/api-keys`
- **THEN** 响应 MUST NOT 含明文，只含前缀与掩码

#### Scenario: 路由不存在
- **WHEN** 对不存在的路由创建或列出 key
- **THEN** 返回 `404`，`code` 为 `not_found`

#### Scenario: 启停
- **WHEN** 已认证用户请求 `PATCH /api/api-keys/{id}` 修改 `status`
- **THEN** 服务更新其状态

#### Scenario: 删除
- **WHEN** 已认证用户请求 `DELETE /api/api-keys/{id}`
- **THEN** 服务删除该 key，返回 `204`

### Requirement: 个人资料与口令

系统 SHALL 允许已认证用户查看与修改自己的资料（昵称 `display_name`、头衔 `title`），并可设置或修改自己的口令。

`GET /api/auth/me` MUST 返回 `display_name`、`title` 与 `has_password`（当前用户的口令哈希是否已脱离锁定态）。

`POST /api/auth/password` MUST 校验当前口令后再设置新口令。

唯一例外：`desktop` 模式下，请求同时携带可验证的 `desktop_token`（SHA-256 与本进程桌面登录令牌一致）且目标用户为 `admin` 时，MUST 允许省略当前口令。`standalone` 模式 MUST NOT 接受该豁免；令牌无效、缺失或调用者非 admin 时 MUST 回落到「必须校验当前口令」。

`desktop_token` MUST NOT 被写入日志、响应或持久化存储。

#### Scenario: 修改资料
- **WHEN** 已认证用户请求 `PATCH /api/auth/me` 并携带昵称 / 头衔
- **THEN** 服务更新该用户资料并返回更新后的用户信息

#### Scenario: 修改口令
- **WHEN** 已认证用户请求 `POST /api/auth/password`，当前口令正确且新口令非空
- **THEN** 服务更新口令（Argon2id），返回 `204`

#### Scenario: 当前口令错误
- **WHEN** 当前口令不正确
- **THEN** 返回 `401`，`code` 为 `invalid_credentials`

#### Scenario: 桌面令牌授权设置口令
- **WHEN** 桌面会话（admin、口令哈希为锁定态）请求 `POST /api/auth/password`，携带 `{ new_password, desktop_token }` 且令牌有效
- **THEN** 服务写入新口令并返回 `204`，随后可用该口令调用 `POST /api/auth/login` 成功

#### Scenario: 设置后状态可读
- **WHEN** 口令设置成功后任意客户端请求 `GET /api/auth/me`
- **THEN** `has_password` 由 `false` 变为 `true`

#### Scenario: 缺少当前口令且无令牌
- **WHEN** `POST /api/auth/password` 只带 `new_password`
- **THEN** 返回 `422`（`invalid_request`），不修改口令

#### Scenario: 独立模式不接受令牌豁免
- **WHEN** `standalone` 模式下请求携带 `desktop_token` 但缺少当前口令
- **THEN** 返回 `422`，不修改口令

#### Scenario: 非 admin 不接受令牌豁免
- **WHEN** 非 admin 用户的会话携带有效 `desktop_token` 但缺少当前口令
- **THEN** 返回 `422`，不修改口令

### Requirement: 用户管理（admin）

系统 SHALL 仅允许 `admin` 角色管理用户；非 admin 请求 MUST 返回 `403`。

端点：`GET /api/users`、`POST /api/users`、`PATCH /api/users/{id}`、`POST /api/users/{id}/password`、`DELETE /api/users/{id}`、`POST /api/users/{id}/archive`、`POST /api/users/{id}/offboard`。

创建用户与重置口令时，若未提供口令，服务端 MUST 生成随机口令并**仅在该次响应中返回一次**；库中 MUST 只存 Argon2id 哈希。

`DELETE /api/users/{id}` MUST 为归档语义，MUST NOT 物理删除用户。归档 MUST 满足：

- 不能归档自己；
- 不能归档最后一个可用 admin（`role='admin' AND disabled=0 AND archived_at IS NULL`）；
- 名下不能仍有未归档 Route。

归档 MUST 设置 `archived_at`、`disabled=1` 并删除该用户全部会话。User 归档 MUST 不可恢复；需要重新使用该身份时必须新建用户。`GET /api/users` MUST 默认只返回未归档用户；`?include_archived=true` MAY 返回归档用户。

Offboard MUST 在单个事务内把该用户全部未归档 Route 转移给指定继任者，然后归档该用户并删除会话；未提供继任者且存在未归档 Route 时 MUST 返回 `409`。

#### Scenario: 非 admin 被拒
- **WHEN** 非 admin 用户请求任一 `/api/users` 端点
- **THEN** 返回 `403`，`code` 为 `forbidden`

#### Scenario: 创建用户返回一次性口令
- **WHEN** admin 创建用户且未提供口令
- **THEN** 服务创建用户并在响应中一次性返回生成的口令

#### Scenario: 重置口令
- **WHEN** admin 请求 `POST /api/users/{id}/password` 且未提供口令
- **THEN** 服务重置口令并一次性返回新口令

#### Scenario: 删除保护
- **WHEN** admin 尝试删除（归档）自己、最后一个可用 admin，或仍有活跃路由的用户
- **THEN** 服务拒绝并返回 `409`

#### Scenario: Offboard
- **WHEN** admin 对用户 A 执行 offboard，继任者为 B
- **THEN** A 的全部未归档 Route 转移给 B，A 被归档，A 的所有会话失效

#### Scenario: 归档用户不能登录
- **WHEN** 已归档用户尝试登录或携带旧会话访问
- **THEN** 返回 `401`

#### Scenario: 归档用户不可恢复
- **WHEN** admin 尝试恢复已归档用户
- **THEN** 系统 MUST NOT 提供恢复接口；需要重新使用该身份时 MUST 新建用户

### Requirement: 系统信息（admin）

系统 SHALL 提供 `GET /api/system` 返回只读运行信息（版本、前端资产信息、数据目录），且仅允许 `admin` 读取；非 admin MUST 返回 `403`。

返回的目录路径 MUST 为绝对路径，且 MUST NOT 包含未展开的 `.` / `..` 段。

#### Scenario: 读取系统信息
- **WHEN** admin 用户请求 `GET /api/system`
- **THEN** 返回只读运行信息，包含已规范化的数据目录绝对路径

#### Scenario: 路径规范化
- **WHEN** 进程以相对 `data_root`（如 `.`）启动，或前端资产目录含 `..`
- **THEN** 系统信息展示规范化后的绝对路径

#### Scenario: 非 admin 被拒
- **WHEN** 非 admin 用户请求 `GET /api/system`
- **THEN** 返回 `403`，`code` 为 `forbidden`

### Requirement: 系统设置（admin）

系统 SHALL 提供数据库驱动的系统设置存储，并仅允许 `admin` 读写；非 admin MUST 返回 `403`。

`GET /api/settings` MUST 返回全部已定义设置（缺省项以默认值补齐）。`PATCH /api/settings` MUST 接受部分更新并做**浅合并**。非法值 MUST 返回 `422`（`code` 为 `invalid_request`）。

设置 MUST 记录最后修改时间与修改人。

#### Scenario: 读取设置
- **WHEN** admin 请求 `GET /api/settings`
- **THEN** 返回设置对象；未显式保存过的字段以默认值补齐

#### Scenario: 部分更新
- **WHEN** admin 请求 `PATCH /api/settings` 只携带部分字段
- **THEN** 服务合并更新并返回更新后的完整设置

#### Scenario: 非 admin 被拒
- **WHEN** 非 admin 请求任一 `/api/settings` 端点
- **THEN** 返回 `403`，`code` 为 `forbidden`

#### Scenario: 非法值
- **WHEN** 提交的值超出允许范围（如 `detail_sample_rate` 不在 0–1）
- **THEN** 返回 `422`，`code` 为 `invalid_request`，且不写入

### Requirement: 用量统计设置

系统 SHALL 提供 `usage` 设置节，字段与默认值：

- `enabled`（布尔，默认 `true`）——是否采集用量（数据面接入后生效）
- `detail_mode`（`off` / `errors` / `all`，默认 `errors`）——逐条明细的保留范围
- `detail_retention_hours`（整数，`0`–`8760`，默认 `24`）
- `detail_sample_rate`（浮点，`0`–`1`，默认 `1.0`）——`detail_mode = all` 时的采样率
- `timezone_offset_minutes`（整数，默认取操作系统当前本地 UTC 偏移，无法判定时 `0`）——**只读**：由服务端决定，用于对齐 hour / day 桶

`timezone_offset_minutes` MUST NOT 通过设置接口修改；提交该字段 MUST 返回 `422`、`code = invalid_request`。首次启动且 DB 无 `usage` 行时，服务 MUST 从操作系统本地 UTC 偏移推导并写入 DB；MUST NOT 读取应用专属环境变量。之后 DB 即唯一事实源。

#### Scenario: 默认值
- **WHEN** 系统首次读取 `usage` 设置且从未保存
- **THEN** 返回上述默认值（时区取操作系统本地 UTC 偏移，无法判定时 `0`）

#### Scenario: 校验范围
- **WHEN** 提交 `detail_mode = "everything"` 或 `detail_sample_rate = 2`
- **THEN** 返回 `422`，不写入

#### Scenario: 时区不可修改
- **WHEN** 提交 `usage.timezone_offset_minutes = 0`
- **THEN** 返回 `422`，提示时区由服务端决定；库中值不变

### Requirement: 数据面配置下发

控制面 SHALL 从 DB 构造数据面配置并下发：路由（id / 名称 / 启用 / 选号策略）、provider 凭据（含解密后的 api_key、`models`、`priority`）、API Key（哈希 / 绑定路由 / 启用）、健康种子、**选号指标种子**、**粘性种子**、运行参数。

provider 的 api_key MUST 在控制面解密后下发；数据面 MUST NOT 持有明文到磁盘或日志。

写操作（路由 / provider 凭据 / API Key / 设置）与启动时 MUST 触发全量下发；下发失败 MUST NOT 阻断管理面写操作（只记录日志）。

**选号指标种子** MUST 在启动时由最近 30 分钟的 `usage_attempt_bucket` 聚合得出（每个 provider 一条：尝试数、成功数、延迟总和与计数），无样本的 provider MUST NOT 产生种子；种子 MUST NOT 覆盖数据面运行期已有的样本。

**粘性种子** MUST 在启动时取自运行期快照（见「运行期状态快照」），过滤已过期条目；运行期的 `apply_config` MUST 不注入粘性与指标种子（避免覆盖数据面已学到的状态）。

#### Scenario: 写操作后即时生效
- **WHEN** 管理员新增一条启用中的 provider 凭据
- **THEN** 该凭据立即可参与路由，无需重启

#### Scenario: 启停即时生效
- **WHEN** 管理员停用某路由或某 API Key
- **THEN** 对应转发入口立即按新状态拒绝

#### Scenario: 启动注入指标种子
- **WHEN** 某 provider 最近 30 分钟在 `usage_attempt_bucket` 里有 100 次尝试、98 次成功、延迟总和 30s、计数 100
- **THEN** 下发的种子为 `samples=100`、`failure_rate=0.02`、`ttfb_ms=300`

#### Scenario: 运行期下发不带种子
- **WHEN** 管理员修改某 provider 的优先级触发全量下发
- **THEN** 本次下发不携带粘性与指标种子

### Requirement: 数据面事件持久化

控制面 SHALL 订阅数据面事件：provider 健康变更 MUST 持久化到 `provider_health`（主键 = provider 凭据 id）；启动时 MUST 把 `provider_health`（含冷却截止时刻）**全量回灌**数据面。

#### Scenario: 健康变更落库
- **WHEN** 数据面把某凭据标记为冷却或退役
- **THEN** `provider_health` 出现/更新对应行（含 `state` / `reason` / `cooldown_until`）

#### Scenario: 回池删除行
- **WHEN** 某凭据探针通过回池
- **THEN** 对应 `provider_health` 行被删除

#### Scenario: 重启回灌
- **WHEN** 进程重启且某凭据仍处于冷却中
- **THEN** 该凭据按原冷却截止时刻继续不可用，不被选中

### Requirement: provider 凭据的模型清单

provider 凭据 SHALL 具备 `models` 字段（`{name, upstream}` 对象数组，必填、可为空数组）。空清单表示该凭据**不服务任何模型**。

写入时 MUST 去除空名并按 `name` 去重；`base_url` MUST 非空、为绝对 `http` / `https` URL、包含 host、不含 query / fragment；其 path MAY 为空或任意兼容前缀（如 `/v1`、`/openai`、`/api/paas/v4`），网关按家族在其后拼接 `/chat/completions`、`/responses` 等具体端点路径；`anthropic_messages` 族在 `base_url` 缺少版本段时由转发目标构造自动补齐 `/v1`（见 `gateway-core`「上游地址构造」）。同一路由下相同 `base_url` + api_key 的重复凭据 MUST 仅记录告警并允许共存。

预设条目的 `base_url` MUST 由该预设的 `base_url_template` 与条目变量值展开后落库，MUST NOT 保存未展开的模板；自定义条目的 `base_url` MUST 按用户输入原样落库（仅去尾斜杠）。系统 MUST NOT 做版本段补齐类改写，补齐只发生在转发目标构造。保存时 MUST NOT 以「缺少版本段」为由返回告警。

**创建时的模型清单自动获取**：`POST /api/routes/{id}/provider-keys` 创建后模型清单为空（请求缺省、空数组，或经写入归一化后为空）且 `credential_kind = api_key` 时，系统 MUST 自动执行一次「模型清单获取」等价逻辑——以该凭据**落库的** `api_formats`、`base_url`、`auth_scheme` 与 api_key，按「模型清单获取」的**家族感知地址构造与形状解析**规则调用上游（分支由落库 `api_formats` 决定，与该端点使用请求草稿值相区分）——并把结果写入该凭据的模型清单，每项为同名映射（`{name: <模型 id>, upstream: <模型 id>}`，按 `name` 去重）。

- 自动获取 MUST 有界等待：**上限 5 秒**，超时 MUST 按失败处理；
- 失败（上游不可达 / 超时 / 非 2xx / 形状不符 / 未返回模型）MUST NOT 阻断创建：凭据正常创建（`201`），模型清单保持为空；失败的分类与错误码沿用「模型清单获取」（如 `unsupported_model_list`）；
- 创建响应 MUST 携带自动获取结果 `autodiscover`：`status` 取 `ok`（已写入 ≥ 1 个模型）/ `failed`（已尝试但写入 0 个，MUST 含可读 `error`）/ `skipped`（未尝试）；`failed` 时 MAY 携带 `error_code`（与「模型清单获取」同一套稳定机器码，如 `unsupported_model_list`），供界面按码翻译失败提示；该字段 MUST 只在创建响应出现，列表与更新响应 MUST NOT 出现；
- MUST NOT 触发：创建请求归一化后模型清单非空、`credential_kind ≠ api_key`（如 `google_service_account`）、以及任何更新（`PATCH`）操作。

自动获取只是便利默认值：最终模型清单为空仍表示该凭据**不服务任何模型**，用户可随时用 `POST /api/models/discover` + 更新补齐或清空，MUST NOT 视为配置错误。

#### Scenario: 根地址可用
- **WHEN** 提交 `base_url = "https://api.deepseek.com"`
- **THEN** 创建成功

#### Scenario: 兼容前缀可用
- **WHEN** 提交 `base_url = "https://api.deepinfra.com/v1/openai"`
- **THEN** 创建成功，转发目标为 `https://api.deepinfra.com/v1/openai/chat/completions`

#### Scenario: 非法 base_url 拒绝
- **WHEN** 提交缺少 scheme、非 `http(s)`、缺少 host 或含 query / fragment 的 base URL
- **THEN** 返回 `422`，`code` 为 `invalid_request`

#### Scenario: 重复凭据告警
- **WHEN** 同一路由下再次提交相同 `base_url` + api_key
- **THEN** 允许保存，并记录告警

#### Scenario: 版本段缺失告警
- **WHEN** 提交 `api_formats = ["anthropic_messages"]` 且 `base_url = "https://api.anthropic.com"`（path 不以 `/v1` 结尾）
- **THEN** 保存成功且**不再**返回「缺少版本段」类告警（可达性已由转发目标补齐保证）；落库 `base_url` 不变

#### Scenario: 不自动改写
- **WHEN** 提交 `api_formats = ["chat_completions"]` 且 `base_url = "https://proxy.example.com/openai"`
- **THEN** 落库的 `base_url` 与用户输入一致，转发目标为 `https://proxy.example.com/openai/chat/completions`

#### Scenario: 自动拉取填充清单
- **WHEN** 创建 `credential_kind = api_key` 的凭据、`models` 缺省且 `api_formats` 缺省（走 OpenAI 分支），上游 `GET {base_url}/models` 返回 `{"data":[{"id":"gpt-5"}]}`
- **THEN** 返回 `201`，模型清单为 `[{"name":"gpt-5","upstream":"gpt-5"}]`，响应 `autodiscover.status = "ok"`

#### Scenario: 自动拉取失败不阻断创建
- **WHEN** 创建时模型清单为空，上游不可达或返回非 2xx
- **THEN** 返回 `201`、凭据创建成功、模型清单为空，响应 `autodiscover.status = "failed"` 且 `error` 可读

#### Scenario: 未返回模型按失败
- **WHEN** 上游返回 200 但清单中无可解析的模型 id
- **THEN** 返回 `201`、模型清单为空，`autodiscover.status = "failed"`（`error` 说明未获得模型）

#### Scenario: 有界等待
- **WHEN** 上游在 5 秒内未响应
- **THEN** 创建正常完成，模型清单为空，`autodiscover.status = "failed"`（超时）；创建响应延迟有上界

#### Scenario: 提供清单不触发
- **WHEN** 创建请求 `models` 归一化后非空
- **THEN** 不发起任何上游调用，`autodiscover.status = "skipped"`

#### Scenario: 更新不触发
- **WHEN** `PATCH /api/provider-keys/{id}` 更新（含把 `models` 改为空）
- **THEN** 不自动拉取，更新响应不含 `autodiscover`

#### Scenario: 服务账号凭据不触发
- **WHEN** 创建 `credential_kind = google_service_account` 的凭据且 `models` 为空
- **THEN** 返回 `201`、模型清单为空，`autodiscover.status = "skipped"`，不发起任何上游调用

#### Scenario: 自动拉取透传错误码
- **WHEN** 创建 anthropic 族凭据且自动拉取遇上游 `404`
- **THEN** 返回 `201`、`autodiscover.status = "failed"` 且 `error_code = "unsupported_model_list"`，界面据此提示「上游不支持模型清单拉取，请手动添加模型」

#### Scenario: 模板展开后落库
- **WHEN** 以 `azure` 预设创建凭据并提交 `base_url_vars = {"resource": "myres"}`
- **THEN** 落库 `base_url` 为 `https://myres.openai.azure.com`，不含 `{resource}` 占位符

### Requirement: 模型清单获取

系统 SHALL 提供 `POST /api/models/discover`：按凭据的**协议族**构造上游列模型地址，调用上游并解析模型 id 列表。获取失败 MUST NOT 视为请求失败：MUST 返回 `200`、空清单与可读原因，允许调用方先以空清单保存。

**目标家族与地址构造**：按请求 `api_formats`（表单草稿；缺省时按 provider 预设推断，仍缺省 MUST 视为含 `chat_completions`）按下表优先级取**一个**分支：

| `api_formats` 命中 | 分支 | 上游列模型地址 |
| --- | --- | --- |
| 含 `chat_completions` 或 `responses` | OpenAI | `GET {base_url}/models`（MUST NOT 补版本段） |
| 否则含 `anthropic_messages` | Anthropic | `GET {normalize(base_url)}/models`（缺版本段补 `/v1`） |

`normalize` MUST 复用 `gateway-core`「上游地址构造」的版本段归一化判定（path 中存在 `v1` / `v1beta` / `v1alpha` 段则不补，段名精确比对），且 MUST 只作用于本次请求的地址构造，MUST NOT 回写任何存储值。多族混声明按表中优先级取一：OpenAI 兼容的列模型约定覆盖面最广，Anthropic 兼容实现普遍未提供列模型端点。

**响应解析**：MUST 接受 OpenAI / Anthropic list 形状：`data: [{id, …}]`，取 `id`（Anthropic 形状的 `display_name` 等字段 MUST 忽略）。

形状不命中 MUST 视为**形状不符**。返回的模型 id 即模型清单候选，调用方按 `{name: <模型 id>, upstream: <模型 id>}` 同名写入。

**失败语义**：失败 MUST 分两类——

- **上游不支持模型清单拉取**：上游返回 `404` / `405` / `501`，或响应**形状不符**时，MUST 携带 `error_code = "unsupported_model_list"`，`error` MUST 给明确文案「上游不支持模型清单拉取，请手动添加模型」；
- 其余失败（不可达、超时、`401` / `403`、其他非 2xx）：MUST 给可读 `error`（如「上游返回 {status}」），MUST NOT 标注为「不支持」——鉴权 / 可用性错误不是「不支持」，MUST NOT 误导用户放弃凭据排查。

失败响应 MUST 为 `200`、`models: []` 与 `error`。`error_code` 是稳定机器码（仅需要界面按码翻译时携带）：界面 MUST 按 `errors.<code>` 查 i18n（`zh-CN` / `en-US`），无匹配时回退 `error` 文案。

请求 MAY 提供 `api_key`（表单中新填 / 轮换的未保存凭据）与 `provider_key_id`（引用某条已保存的 provider 凭据）。api_key 的解析 MUST 按以下优先级：

1. 请求 `api_key` 非空 → 使用请求值；
2. 否则请求带 `provider_key_id` → 使用该 Provider **已保存**的 api_key，MUST 校验归属（非当前用户名下的记录 MUST 返回 `404`，MUST NOT 以任何方式取用其凭据）；
3. 两者皆无 → 返回 `200`、`models: []` 与可读 `error`（缺少凭据），MUST NOT 以空凭据发起上游调用。

`base_url`、`auth_scheme` 与 `api_formats` MUST 取自请求（表单草稿值），MUST NOT 从已保存记录回读；`auth_scheme` / `api_formats` 缺省时按 provider 预设推断。

#### Scenario: 成功获取
- **WHEN** 上游返回 `{"data":[{"id":"gpt-5"}]}`
- **THEN** 返回 `models: ["gpt-5"]`

#### Scenario: 获取失败回空
- **WHEN** 上游不可达或返回非 2xx
- **THEN** 返回 `200` 且 `models: []` 并带 `error` 说明

#### Scenario: 编辑态使用已存凭据
- **WHEN** 编辑已有 Provider 时请求 `{"provider_key_id": "pk_1", "base_url": "https://api.example.com/v1", "api_key": ""}`
- **THEN** 用 `pk_1` 已保存的 api_key 调用上游 `GET https://api.example.com/v1/models` 并返回模型清单

#### Scenario: 请求凭据优先
- **WHEN** 请求同时带非空 `api_key` 与 `provider_key_id`
- **THEN** 使用请求中的 api_key 发起调用，不使用已存凭据

#### Scenario: 越权引用被拒绝
- **WHEN** `provider_key_id` 不属于当前用户
- **THEN** 返回 `404`，不发起任何上游调用

#### Scenario: 无凭据
- **WHEN** 请求 `api_key` 为空且未提供 `provider_key_id`
- **THEN** 返回 `200`、`models: []` 与可读 `error`，不发起任何上游调用

#### Scenario: Anthropic 分支地址与形状
- **WHEN** `api_formats = ["anthropic_messages"]`、`base_url = "https://api.example.com/apps/anthropic"`，上游 `GET https://api.example.com/apps/anthropic/v1/models` 返回 `{"data":[{"id":"claude-x","display_name":"Claude X"}]}`
- **THEN** 请求命中补齐 `/v1` 后的地址，返回 `models: ["claude-x"]`（`display_name` 忽略）

#### Scenario: Anthropic 已含版本段不重复补
- **WHEN** `api_formats = ["anthropic_messages"]`、`base_url = "https://api.anthropic.com/v1"`
- **THEN** 请求 `https://api.anthropic.com/v1/models`，`/v1` 只出现一次

#### Scenario: 已移除的 Gemini 分支
- **WHEN** `api_formats = ["gemini_generate_content"]`、`base_url = "https://generativelanguage.googleapis.com"`（该取值已移除）
- **THEN** 请求按 OpenAI 分支构造 `GET https://generativelanguage.googleapis.com/models`；上游返回 `{"models":[{"name":"models/gemini-2.5-pro"}]}` 视为形状不符，返回 `error_code = "unsupported_model_list"`

#### Scenario: OpenAI 分支不补版本段
- **WHEN** `api_formats = ["chat_completions"]`、`base_url = "https://api.deepseek.com"`
- **THEN** 请求 `https://api.deepseek.com/models`，MUST NOT 补 `/v1`

#### Scenario: 多族混声明优先 OpenAI 分支
- **WHEN** `api_formats = ["chat_completions", "anthropic_messages"]`、`base_url = "https://proxy.example.com/x"`
- **THEN** 请求 `https://proxy.example.com/x/models`（不补版本段），按 OpenAI / Anthropic list 形状解析

#### Scenario: 形状不符给明确错误
- **WHEN** 上游返回 200 但响应体不命中 `data[].id` 形状（如 HTML、`{"error":{…}}` 或 Gemini `{"models":[…]}`）
- **THEN** 返回 `200`、`models: []`，`error_code = "unsupported_model_list"`，`error` 为「上游不支持模型清单拉取，请手动添加模型」

#### Scenario: 404 视为不支持
- **WHEN** 上游返回 `404`
- **THEN** 返回 `200`、`models: []`，`error_code = "unsupported_model_list"` 与上述明确文案

#### Scenario: 鉴权错误不标不支持
- **WHEN** 上游返回 `401`
- **THEN** 返回 `200`、`models: []` 与可读 `error`（如「上游返回 401」），MUST NOT 携带 `unsupported_model_list`

**凭据类型限制**：`credential_kind = google_service_account` 的记录 MUST 不支持该能力。请求以 `provider_key_id` 引用此类记录时 MUST 返回 `200` + 空清单 + `error_code = "unsupported_credential_kind"`，MUST NOT 发起上游请求，MUST NOT 把凭据内容当作 Bearer token 发送。

#### Scenario: Service Account 不支持
- **WHEN** 请求以 `provider_key_id` 引用 `credential_kind = google_service_account` 的记录
- **THEN** 返回空清单且 `error_code = unsupported_credential_kind`，MUST NOT 发起上游请求

### Requirement: 转发参数设置（admin）

系统 SHALL 在 DB 设置中提供 `gateway` 节：`cooldown_auth_secs`、`cooldown_rate_limit_secs`、`cooldown_unavailable_secs`、`quota_cooldown_secs`（新增，配额耗尽类失败的长冷却，默认 `21600`）、`affinity_ttl_secs`（仅 admin 可改，非法值 `422`）。保存后 MUST 即时下发给数据面。

#### Scenario: 默认值
- **WHEN** 从未保存过 `gateway` 设置
- **THEN** 返回 1800 / 300 / 60 / 21600 / 86400 秒

#### Scenario: 修改即时生效
- **WHEN** admin 修改 `cooldown_auth_secs`
- **THEN** 保存成功且新的冷却时长对后续失败立即生效

#### Scenario: 配额冷却可调
- **WHEN** admin 把 `quota_cooldown_secs` 改为 7200
- **THEN** 之后发生的配额类失败按 7200 秒冷却

### Requirement: 用量采集与持久化

控制面 SHALL 订阅数据面事件并采集用量：`Access` 累加请求数、换号、TTFB、请求体字节与状态计数；`Completion` 累加完成 / 中断 / 流错误、总耗时、响应体字节与 token。

聚合桶 MUST 使用 `(granularity, bucket_ts, route_id, api_key_id, model, provider_id)` 作为唯一键，支持 `hour` / `day` 两档。`provider_id` MUST 表示最终成功承接请求的 Provider；无承接时为空。

聚合 MUST 先在内存累加并定期批量落盘，SHALL NOT 为每个请求单独写库。token MUST 写入聚合桶，SHALL NOT 依赖逐条明细保留。

逐条明细保留策略：`detail_mode = off` 不记；`errors` 记 `status >= 400` **或发生过换号**（`attempts > 1`）的请求；`all` 记全部并按 `detail_sample_rate` 采样。明细 MUST 在完成事件后补充 token、响应体字节与 outcome。

`usage.enabled = false` 时 MUST 停止累计，且变更 MUST 即时生效（无需重启）。

**落盘失败 MUST NOT 丢弃增量**：写库失败的增量 MUST 合并回内存缓冲并在后续周期重试；连续失败时 MUST 限频告警。

**逐条明细 MUST 走独立的写入路径**：先进入内存缓冲，每 2–3 秒用一个事务批量落库；同一次请求的 `Access` 与 `Completion` 事件 MUST 合并为一行（顺序无关）；落库失败 MUST 回灌缓冲重试。

进程收到关闭信号时 MUST 先执行一次落盘（聚合桶与明细）再退出（见 `platform-support` 的「优雅关闭」）。

#### Scenario: 聚合落盘
- **WHEN** 若干请求完成后等待落盘周期
- **THEN** 对应时间桶的请求数、错误数、换号数、延迟直方图、token 与字节数被累加

#### Scenario: Provider token 归属
- **WHEN** 一次请求经过换号后由 Provider B 成功承接
- **THEN** 该请求的 token MUST 计入 Provider B，而不是失败尝试的 Provider A

#### Scenario: 明细按设置保留
- **WHEN** `detail_mode = errors` 且请求成功且未换号
- **THEN** 不产生明细行；请求失败时产生明细行

#### Scenario: 换号成功也留明细
- **WHEN** `detail_mode = errors`，请求先 429 换号、最终 200 成功
- **THEN** 产生一行明细，`failovers >= 1`，`attempts_json` 含两次尝试

#### Scenario: 明细批量落库
- **WHEN** 一个批次内有 3 个请求的 Access 与 Completion 事件
- **THEN** 一个事务内写入 3 行（含 token 与 outcome），不产生每请求事务

#### Scenario: 事件顺序无关
- **WHEN** `Completion` 先于 `Access` 到达缓冲，或 `Access` 已落库后 `Completion` 才到
- **THEN** 最终仍是同一行，且 token / outcome 被补齐

#### Scenario: 明细落库失败重试
- **WHEN** 一次明细批量写入失败
- **THEN** 该批明细回到缓冲，下个周期重试，不丢失

#### Scenario: 关闭采集即时生效
- **WHEN** admin 关闭 `usage.enabled`
- **THEN** 之后完成的请求不再累计

#### Scenario: 落库失败重试
- **WHEN** 一次聚合落盘因 DB 报错失败
- **THEN** 该批增量回到内存缓冲，下个周期再次尝试，统计不丢失

#### Scenario: 关闭前落盘
- **WHEN** 进程收到 SIGTERM
- **THEN** 退出前把聚合缓冲与明细缓冲各落盘一次

### Requirement: 用量查询

系统 SHALL 提供只读用量查询接口：

- `GET /api/usage/summary`：请求数、完成数、错误数、错误率、换号数、平均延迟、TTFB、P50/P95/P99（直方图近似）、字节数、各 token 数量
- `GET /api/usage/timeseries`：按 `interval`（hour/day）的时间序列，包含请求、错误、token、字节与延迟汇总
- `GET /api/usage/breakdown`：按 `by`（route/api_key/model/provider）分组，Provider 分组 MUST 返回 token
- `GET /api/usage/requests`：保留期内的请求明细，含 model、provider、token 与 outcome
- `GET /api/usage/config`：返回 `timezone_offset_minutes`，供控制台按统一时区换算自定义日期；任意已认证用户可读

所有 `/api/usage/*` 接口 MUST 限制为当前用户拥有的路由。未提供 `route_id` 时 MUST 自动按 `routes.owner_id = 当前用户` 过滤；提供 `route_id` 时 MUST 校验该路由属于当前用户，否则返回 `404`。`admin` MUST NOT 通过该接口查询他人路由或做全站聚合。

时间参数 MUST 支持 `range = 24h | today | yesterday | 7d | 30d | this_month | last_month | custom`，`custom` 时 MUST 提供 `from` / `to`（Unix 秒）。「今天 / 昨天 / 近 7 天 / 近 30 天 / 本月 / 上月」MUST 按 `usage.timezone_offset_minutes` 的本地日历计算；该偏移是采集与查询共用的唯一口径，MUST NOT 由单次请求覆盖。

粒度 MUST 按跨度自动选择：≤90d 使用 `hour`，超过 90d 使用 `day`；`interval=hour` 必须落在 hour 保留期内，`interval=day` 可用于短窗口。时间序列点数 SHOULD 限制在 500 以内并在超出时按 N 小时 / N 天降采样。

#### Scenario: 汇总
- **WHEN** 用户请求 `/api/usage/summary` 且窗口内有数据
- **THEN** 返回请求数、错误率、token、百分位延迟等汇总值

#### Scenario: 无数据
- **WHEN** 窗口内无任何数据
- **THEN** 返回零值而非错误

#### Scenario: 时间序列粒度
- **WHEN** 请求 `range=this_month`
- **THEN** 按本地时区聚合返回时间点

#### Scenario: 越权路由
- **WHEN** 用户请求 `/api/usage/summary?route_id=<他人路由>`
- **THEN** 返回 `404`

#### Scenario: admin 普通统计仍受 owner 限制
- **WHEN** admin 请求 `/api/usage/summary` 且未指定 `route_id`
- **THEN** 仅返回 admin 自己路由的用量

### Requirement: provider 健康查询

系统 SHALL 提供 `GET /api/provider-health`，返回当前不可用的 provider 凭据及其所属路由、名称、provider、状态（`quarantined` / `retired`）、原因与冷却截止时刻。

响应 MUST 同时提供「是否可探测」的事实：`credential_kind = google_service_account`、模型清单为空、或 `api_formats` 为空的记录 MUST 标记为不可探测。不可探测 MUST NOT 被呈现为健康。

#### Scenario: 列出不可用凭据
- **WHEN** 已认证用户请求 `/api/provider-health`
- **THEN** 返回全部带健康标记的凭据；无标记时返回空数组

#### Scenario: 不可探测标记
- **WHEN** 查询结果中包含 Service Account 凭据或模型清单为空的凭据
- **THEN** 响应标记其为不可探测，控制台 MUST NOT 显示为健康

### Requirement: provider 健康人工恢复

admin SHALL 能清除某 Provider 的健康标记（退役 / 冷却），使其重新参与选号；操作 MUST 立即下发数据面。

人工解除操作（`DELETE /api/provider-health/{provider_id}`）MUST **不受路由 owner 作用域限制**：admin MAY 解除全站任意 Provider 的标记（服务 admin 全站故障页 `/incidents`），该端点 MUST NOT 受「路由子资源访问控制」的 owner 约束（不在其端点列举内）；`GET /api/provider-health` 查询仍 MUST 按 owner 隔离。

人工解除 MUST 同时关闭该 Provider 的未解决故障记录（`resolved_by = manual`）。

#### Scenario: 解除标记
- **WHEN** admin 删除某凭据的健康标记
- **THEN** 该标记被移除，且数据面随即按健康状态重新选号

#### Scenario: 非 admin 被拒
- **WHEN** 非 admin 调用该端点
- **THEN** 返回 `403`，`code` 为 `forbidden`

#### Scenario: admin 解除他人路由的标记
- **WHEN** admin 解除他人路由下 Provider 的健康标记
- **THEN** 返回 `204`，标记被移除，数据面随即按健康状态重新选号

#### Scenario: 解除退役并关闭故障
- **WHEN** admin 对退役的 Provider 执行解除
- **THEN** 健康标记被删除，其 `open` 故障记录全部关闭，`resolved_by=manual`

### Requirement: 配置只来自数据库

`solidway` SHALL NOT 读取任何配置文件。除 `data_root`（命令行参数）与主密钥外的所有可调参数 MUST 来自数据库 `settings` 表；缺失时使用内置默认值。

已废弃的 `<data_root>/config.toml` MUST NOT 被读取，也 MUST NOT 自动创建。

#### Scenario: 无 config.toml 也能启动
- **WHEN** `data_root` 下没有 `config.toml`
- **THEN** 服务正常启动，所有参数取数据库值或内置默认值

#### Scenario: 残留的旧配置被忽略
- **WHEN** `data_root` 下存在旧的 `config.toml`
- **THEN** 服务正常运行且不读取其中任何字段

### Requirement: 会话与安全设置

`auth` 设置 MUST 包含 `session_ttl_hours`（范围 1..=720，默认 `24`）与 `cookie_secure`（默认 `false`）。

`cookie_secure` MUST NOT 出现在管理界面的任何可编辑字段中；只能通过直接修改数据库变更。

#### Scenario: 默认会话时长
- **WHEN** 数据库中没有任何 `auth` 设置
- **THEN** 登录签发的会话有效期为 24 小时

#### Scenario: 无法从界面修改 cookie_secure
- **WHEN** 管理员打开设置页
- **THEN** 不存在 `cookie_secure` 的开关或输入框

### Requirement: 主密钥来源

主密钥 MUST 按以下优先级解析：命令行 `--master-key <hex|base64>` → `<data_root>/master.key` → 首次启动自动生成并写入 `<data_root>/master.key`。

环境变量 `SOLIDWAY_MASTER_KEY` MUST NOT 被读取。

#### Scenario: 命令行内联密钥
- **WHEN** 以 `--master-key <32 字节的 hex 或 base64>` 启动
- **THEN** 使用该密钥解密 provider 凭据，且不读取也不改写 `master.key` 文件

#### Scenario: 首次生成
- **WHEN** 既没有 `--master-key` 也没有 `master.key`
- **THEN** 生成新密钥写入 `<data_root>/master.key` 并打印提示

#### Scenario: 密钥格式非法
- **WHEN** `--master-key` 既不是 32 字节 hex 也不是 32 字节 base64
- **THEN** 启动失败并说明期望格式

### Requirement: 网络设置

`network` 设置 MUST 包含 `bind_addrs`（地址列表，默认 `["127.0.0.1"]`）、`port`（默认 `6565`，范围 1..=65535）、`connect_timeout_secs`（默认 10）、`first_byte_timeout_secs`（默认 60），后两者范围 1..=3600。

`bind_addr`（单地址字符串）MUST 保留为**只读兼容字段**，取值等于 `bind_addrs` 的首个元素；写入时它 MAY 被当作单元素列表的简写。

`bind_addrs` 校验：每项 MUST 是合法 IP、去重后非空、最多 16 项；出现通配符（`0.0.0.0` / `::`）时 MUST 只允许该项存在；违规 MUST 返回 `422`。

`port` MUST NOT 接受 `0`。两个超时 MUST 在保存后对新请求立刻生效；`bind_addrs` / `port` 的改动 MUST 在重启后生效，且 `restart_required` MUST 按"配置地址集合与实际监听集合是否相等"判定。

#### Scenario: 超时即时生效
- **WHEN** 管理员把 `connect_timeout_secs` 从 10 改为 5 并保存
- **THEN** 后续转发请求使用 5 秒连接超时，无需重启

#### Scenario: 端口需重启
- **WHEN** 管理员修改 `port` 并保存
- **THEN** 服务仍监听旧端口，接口返回 `restart_required = true`

#### Scenario: 拒绝端口 0
- **WHEN** 请求把 `port` 设为 `0`
- **THEN** 返回 400，说明端口必须为 1..=65535

#### Scenario: 多地址保存
- **WHEN** 提交 `bind_addrs = ["127.0.0.1", "192.168.1.10"]`
- **THEN** 保存成功，`restart_required = true`，重启后两个地址均监听同一端口

#### Scenario: 通配符不可混用
- **WHEN** 提交 `bind_addrs = ["0.0.0.0", "127.0.0.1"]`
- **THEN** 返回 422，提示通配符不能与其他地址同时使用

#### Scenario: 旧字段兼容
- **WHEN** 请求只提交 `bind_addr = "127.0.0.1"`
- **THEN** 等效于提交 `bind_addrs = ["127.0.0.1"]`

### Requirement: 运行模式与重启

服务 MUST 支持 `--mode <desktop|standalone>`（默认 `standalone`）。

`GET /api/app/info` MUST 只返回界面所需信息：`version`、`mode`、`can_restart`、`listen{effective, source, restart_required}`；MUST NOT 包含 `data_root`、资产路径或进程信息。该端点 MUST 仅允许 `admin` 读取；非 admin MUST 返回 `403`。

`POST /api/app/restart` MUST 仅在 `desktop` 模式下可用（返回 202），否则返回 409 `restart_unsupported`。

#### Scenario: 桌面模式下可重启
- **WHEN** `--mode desktop` 且管理员调用 `POST /api/app/restart`
- **THEN** 返回 202，随后进程退出以便托管方重启

#### Scenario: 独立模式拒绝重启
- **WHEN** `--mode standalone`（默认）且调用 `POST /api/app/restart`
- **THEN** 返回 409，提示需手动重启

### Requirement: 监听命令行覆盖

`--listen <addr>` MUST 支持 `host:port`、`:port`、裸 `port` 三种写法，优先级高于数据库。

被覆盖时 `GET /api/app/info` 的 `listen.source` MUST 为 `command_line`，且 `restart_required` 恒为 `false`。

即使 `--listen` 覆盖生效，`PATCH /api/settings` 仍 MUST 允许更新 `network` 的 `bind_addr` 与 `port`；修改后的值 MUST 持久化到数据库，并在下次不带 `--listen` 启动时生效。

#### Scenario: 覆盖端口
- **WHEN** 数据库 `port = 6565`，以 `--listen :7000` 启动
- **THEN** 实际监听 7000，`source = "command_line"`

#### Scenario: 覆盖期间仍可修改数据库目标值
- **WHEN** 以 `--listen :7000` 启动后，管理员在界面修改 `port` 并保存
- **THEN** 新值写入数据库；当前进程仍监听 7000，下次不带 `--listen` 启动时使用新端口

### Requirement: 前端静态资源来源

服务 MUST 按以下优先级解析前端静态资源：`--web-assets <dir>` → 编译期默认（debug 构建读 `web/dist`，release 构建使用内置资产）。

系统信息接口 MUST 暴露 `web_assets { source, path, files }` 以便诊断。

`--web-assets` 指定目录缺少 `index.html` 时 MUST 启动失败，MUST NOT 静默回退。

#### Scenario: 开发构建读磁盘
- **WHEN** 以 debug 构建启动且未指定 `--web-assets`
- **THEN** 从仓库的 `web/dist` 读取，修改前端无需重编 Rust

#### Scenario: 发布构建用内置
- **WHEN** 以 release 构建启动且未指定 `--web-assets`
- **THEN** 使用编译进二进制的资产，`web_assets.source = "embedded"`

#### Scenario: 覆盖目录非法
- **WHEN** `--web-assets /tmp/nope` 且该目录无 `index.html`
- **THEN** 启动失败并说明原因

### Requirement: 不依赖工作目录

静态资源解析 MUST NOT 依赖进程当前工作目录；任意 cwd 启动行为一致。

#### Scenario: 从任意目录启动
- **WHEN** 在 `/tmp` 下启动 debug 构建
- **THEN** 仍能从仓库 `web/dist` 提供控制台页面

### Requirement: 单页应用回退

未知的非 `/api`、非 `/v1` 路径 MUST 回退到 `index.html`；`index.html` MUST 返回 `Cache-Control: no-cache`，带内容哈希的 `assets/*` MUST 返回长缓存。

#### Scenario: 深链接刷新
- **WHEN** 直接访问 `/routes/abc` 并刷新
- **THEN** 返回 `index.html` 内容，状态码 200

### Requirement: 引导设置口令接口

系统 SHALL 提供公开接口完成 admin 引导：

- `GET /api/auth/bootstrap` MUST 返回 `{ "pending": bool }`，表示是否存在待用引导令牌，且 MUST NOT 泄露令牌任何信息。
- `POST /api/auth/bootstrap` MUST 接受 `{ "token", "new_password" }`，校验通过后写入 admin 口令哈希、删除引导令牌，并**直接签发会话**（响应与 `POST /api/auth/login` 一致）。

`new_password` 长度不足 8 MUST 返回 `422`，`code` 为 `invalid_request`。

令牌无效、缺失、已消费，或目标用户已不处于锁定态时 MUST 返回 `401`，`code` 为 `invalid_bootstrap`。

#### Scenario: 设置成功即已登录
- **WHEN** 客户端携有效令牌与合法新口令请求 `POST /api/auth/bootstrap`
- **THEN** 服务写入口令、删除令牌，返回 `200`、`MeResponse` 与 HttpOnly 会话 Cookie

#### Scenario: 令牌重复使用
- **WHEN** 客户端用同一令牌第二次请求 `POST /api/auth/bootstrap`
- **THEN** 服务返回 `401`，`code` 为 `invalid_bootstrap`

#### Scenario: 口令过短
- **WHEN** 新口令长度小于 8
- **THEN** 服务返回 `422`，`code` 为 `invalid_request`，且令牌不被消费

#### Scenario: 查看是否待引导
- **WHEN** 客户端请求 `GET /api/auth/bootstrap`
- **THEN** 服务返回 `200` 与 `{ "pending": <bool> }`，响应中不含令牌或其哈希

### Requirement: 引导令牌的交付通道

引导令牌 MUST 只经标准输出事件交付，格式 MUST 为单行 `solidway: bootstrap token=<token> url=<完整引导 URL>`；`url` MUST 指向 `/bootstrap#<token>` 且使用 bind 后的实际监听地址。

该方式对 `--mode desktop` 与 `--mode standalone` MUST 完全一致：引导逻辑与接口 MUST NOT 区分运行模式。

服务端 MUST NOT 通过 `tracing` 或任何文件写入令牌。桌面壳 MUST NOT 把该前缀的 stdout 行写入 `<data_root>/solidway.log`。

#### Scenario: 事件自带可点开的 URL
- **WHEN** 首次启动绑定到 `127.0.0.1:6565`
- **THEN** stdout 出现 `solidway: bootstrap token=<token> url=http://127.0.0.1:6565/bootstrap#<token>`

#### Scenario: 两种模式输出一致
- **WHEN** 分别以 `--mode desktop` 与 `--mode standalone` 对同一空白 `data_root` 启动
- **THEN** 两者的引导事件行格式相同，接口行为相同；差异仅在消费方（壳 vs 终端）

#### Scenario: 日志中不含令牌
- **WHEN** desktop 模式首启后检查 `<data_root>/solidway.log`
- **THEN** 日志中不含引导令牌明文

#### Scenario: 数据库不含令牌
- **WHEN** 首启后检查 `<data_root>/solidway.db`
- **THEN** 数据库中没有承载引导令牌的表，库文件字节中也不含令牌明文或其哈希

### Requirement: CLI 重置 admin 口令

`solidway` SHALL 接受 `--reset-password` 选项：把 `admin` 的口令哈希置回锁定态（不可登录）、作废其全部会话与待用引导令牌，然后退出（成功码 `0`）。

该流程 MUST NOT 启动服务、MUST NOT 加载主密钥、MUST NOT 打印任何凭证；新的引导令牌 MUST 由复位后的下一次启动签发。

#### Scenario: 重置后可重新引导
- **WHEN** 对已可用的实例执行 `solidway <data_root> --reset-password`，随后再次启动
- **THEN** 旧口令登录返回 `401`，本次启动签发新的引导令牌并可完成引导

#### Scenario: 重置不占端口
- **WHEN** 已有实例占用监听端口时执行 `--reset-password`
- **THEN** 重置成功退出，不报端口冲突

### Requirement: 版本接口

系统 SHALL 提供 `GET /api/version`，对**任意已认证角色**返回 `{ "version": "<语义化版本>" }`。

响应 MUST 只包含 `version` 一个字段；`data_root`、监听地址、静态资源来源等运行事实 MUST 继续只由 `GET /api/system` 与 `GET /api/app/info` 提供，且仅 `admin` 可读。

未认证请求 MUST 返回 `401`，`code` 为 `unauthorized`。

#### Scenario: 普通用户可读版本
- **WHEN** role 为 `user` 的已认证用户请求 `GET /api/version`
- **THEN** 返回 `200` 与 `{ "version": ... }`，且响应体不含其它字段

#### Scenario: 未认证
- **WHEN** 未携带会话 Cookie 请求 `GET /api/version`
- **THEN** 返回 `401`，`code` 为 `unauthorized`

#### Scenario: 运行事实仍仅 admin
- **WHEN** role 为 `user` 的用户请求 `GET /api/system` 或 `GET /api/app/info`
- **THEN** 返回 `403`，`code` 为 `forbidden`

### Requirement: API Key 有效期

API Key SHALL 支持可选 `expires_at`（Unix 秒，NULL = 永不过期）。

创建时若提供 `expires_at`，其 MUST 晚于当前时间；否则 MUST 返回 `422`、`code = invalid_request`。缺省或 null MUST 视为永不过期。

更新 MUST 支持设置/延长/清空：字段缺省 = 不修改；JSON `null` = 清空（永不过期）；数值 = 新的到期时间。

列表、创建与更新响应 MUST 返回 `expires_at`。

#### Scenario: 创建带有效期的 Key
- **WHEN** 创建时提交未来的 `expires_at`
- **THEN** 保存并回显该值

#### Scenario: 过去的到期时间
- **WHEN** 创建时提交 `expires_at <= now`
- **THEN** 返回 `422`，`code = invalid_request`

#### Scenario: 清空有效期
- **WHEN** PATCH 提交 `{"expires_at": null}`
- **THEN** 该 Key 变为永不过期

#### Scenario: 不修改
- **WHEN** PATCH 未包含 `expires_at`
- **THEN** 原到期时间保持不变

### Requirement: TLS 设置模型

系统 SHALL 在 DB 设置中提供 `tls` 节，包含：`enabled`（默认 `false`）、`email`、`challenge`（默认 `http-01`）、`directory`（默认 `production`）、`http_port`（默认 `80`）、`https_port`（默认 `443`）、`hsts`（默认 `false`）。系统 MUST NOT 依赖任何配置文件提供 TLS 设置。

`http_port` / `https_port` MUST 是**本地监听端口**：系统 MUST NOT 因端口不是 80 / 443 而拒绝启用，也 MUST NOT 假设公网端口已映射（见 `platform-support` 的「ACME 挑战端口语义」）。

证书域名 MUST 直接来自 `access.public_origins` 的 host：仅保留合法 DNS 名（至少包含一个点、非 `localhost`、非 IP、无端口 / 路径 / 通配符），去重；不符合的项 MUST 跳过，并在状态中返回原因。

#### Scenario: 默认值
- **WHEN** 读取设置且从未配置过 TLS
- **THEN** `enabled=false`、`challenge=http-01`、`directory=production`、端口 80/443、`hsts=false`

#### Scenario: IP / localhost 被跳过
- **WHEN** `public_origins` 同时包含域名、IP 与 `localhost`
- **THEN** 只有域名参与证书；状态中列出被跳过的项及原因

### Requirement: TLS 启用校验

启用（`enabled=true`）时 MUST 满足：至少一个合法域名、`email` 非空、端口在 1–65535 且互不冲突、`challenge` 与 `directory` 取值合法；否则 MUST 返回 `422`、`code = invalid_request`。

#### Scenario: 没有合法域名
- **WHEN** `public_origins` 只有 IP，用户尝试启用
- **THEN** 返回 `422`，提示缺少合法 DNS 域名

#### Scenario: 端口冲突
- **WHEN** `http_port` 与 `https_port` 相同
- **THEN** 返回 `422`

### Requirement: TLS 证书人工重试

系统 SHALL 提供 `POST /api/tls/retry`（仅 admin）：立即触发一次签发 / 续期尝试，清除 `paused` 状态并重置失败计数。响应 MUST 返回触发结果（是否已受理），MUST NOT 同步等待 ACME 往返完成。

#### Scenario: 触发重试
- **WHEN** admin 在 `paused` 状态下调用 `POST /api/tls/retry`
- **THEN** 返回 202 或 200 表示已受理，稍后状态可从 `GET /api/tls/status` 观察

#### Scenario: 非 admin
- **WHEN** 普通用户调用重试接口
- **THEN** 返回 `403`

### Requirement: TLS 变更与 HSTS

`enabled`、端口、`challenge`、`directory` 变更 MUST 要求重启监听器，并在保存响应中返回 `restart_required=true`；证书续期 MUST NOT 要求重启。

`hsts` 默认 MUST 为 `false`；为 `true` 时 MUST 发送 HSTS 响应头。

#### Scenario: 修改监听设置
- **WHEN** admin 修改 `enabled` 或端口并保存
- **THEN** 保存成功并返回 `restart_required=true`

#### Scenario: 人工重试
- **WHEN** `cert.state=paused` 时 admin 触发重试
- **THEN** 立即再次尝试签发，MUST NOT 要求重启进程

#### Scenario: HSTS 默认关闭
- **WHEN** 未显式开启 `hsts`
- **THEN** 响应不包含 HSTS 头

### Requirement: TLS 状态接口

系统 SHALL 提供 `GET /api/tls/status`（仅 admin），返回当前模式、challenge、directory、参与证书的域名、被跳过的 origin、证书状态、到期时间、上次续期时间、最后错误、`restart_required`、`challenge_port_hint`。

`cert.state` MUST 为 `ok` / `waiting` / `paused` 之一（分别表示可用、等待签发、连续失败后暂停自动重试）。`challenge_port_hint` 在 challenge 所需公网端口（`http-01` 为 80、`tls-alpn-01` 为 443）与对应本地端口不一致时 MUST 返回可读提示，否则为 `null`。

#### Scenario: 查看状态
- **WHEN** admin 请求 `GET /api/tls/status`
- **THEN** 返回上述字段；未启用时 `enabled=false`，证书字段为空

#### Scenario: 非 admin
- **WHEN** 普通用户请求状态接口
- **THEN** 返回 `403`

#### Scenario: 端口提示
- **WHEN** `challenge=http-01` 且 `http_port=8080`
- **THEN** 状态返回 `challenge_port_hint`，说明需要 80→8080 映射

#### Scenario: 暂停状态
- **WHEN** 同一证书 24 小时内失败 5 次
- **THEN** `cert.state=paused`，`last_error` 携带最后一次失败原因

### Requirement: Provider 鉴权方式

系统 SHALL 在 provider key 上保存 `auth_scheme`，取值限定为 `bearer` / `x-api-key` / `api-key` / `google_service_account` / `sigv4` / `none`。

`x-goog-api-key` MUST NOT 作为 `auth_scheme`；提供该值 MUST 返回 `422`、`code = invalid_request`。

预设条目的 `auth_scheme` MUST 取预设值，请求 MUST NOT 覆盖；携带与预设不一致的值 MUST 返回 `422`、`code = preset_field_immutable`。自定义条目未提供时 MUST 默认 `bearer`；显式提供时 MUST 校验取值，未知取值 MUST 返回 `422`、`code = invalid_request`。`credential_kind = google_service_account` 时 MUST 为 `google_service_account`；`credential_kind = api_key` 时 MUST NOT 使用该值（`422`、`code = incompatible_auth_scheme`）。`credential_kind = aws_sigv4` 时 MUST 为 `sigv4`；`sigv4` MUST NOT 与其它凭据类型组合。

下发给数据面时 MUST 使用记录上的鉴权方式。`google_service_account` MUST 使用凭据 JSON 换取 OAuth2 访问令牌，MUST NOT 读取 ADC、元数据服务或环境变量。`sigv4` MUST 按 `gateway-core`「SigV4 请求签名」对每个转发请求签名。

#### Scenario: 预设默认
- **WHEN** 以 `azure` 预设创建且未提供 `auth_scheme`
- **THEN** 保存并回显 `api-key`

#### Scenario: 预设默认（google-vertex-anthropic）
- **WHEN** 以 `google-vertex-anthropic` 预设创建且未提供 `auth_scheme`
- **THEN** 保存并回显 `google_service_account`（原 `google-vertex` 预设已移除）

#### Scenario: 预设默认（amazon-bedrock）
- **WHEN** 以 `amazon-bedrock` 预设创建且未提供 `auth_scheme`
- **THEN** 保存并回显 `sigv4`

#### Scenario: 自定义默认
- **WHEN** 以目录未命中的 provider 创建且未提供 `auth_scheme`
- **THEN** 保存并回显 `bearer`

#### Scenario: 显式覆盖
- **WHEN** 以目录未命中的 provider 创建时提供 `auth_scheme = "x-api-key"`
- **THEN** 原样保存并回显该值

#### Scenario: 未知取值
- **WHEN** 提供 `auth_scheme = "oauth"` 或 `"x-goog-api-key"`
- **THEN** 返回 `422`、`code = invalid_request`，不落库

#### Scenario: 预设锁定
- **WHEN** 以 `azure` 预设创建并提交 `auth_scheme = "bearer"`
- **THEN** 返回 `422`、`code = preset_field_immutable`，落库值为预设的 `api-key`

#### Scenario: 与凭据类型冲突
- **WHEN** 自定义条目提供 `credential_kind = api_key` 与 `auth_scheme = "google_service_account"`
- **THEN** 返回 `422`、`code = incompatible_auth_scheme`

#### Scenario: sigv4 与凭据类型冲突
- **WHEN** 自定义条目提供 `credential_kind = api_key` 与 `auth_scheme = "sigv4"`
- **THEN** 返回 `422`、`code = incompatible_auth_scheme`

### Requirement: Provider 凭据类型

系统 SHALL 在 provider key 上保存 `credential_kind`（`api_key` / `google_service_account` / `aws_sigv4`）以及对应密文、掩码与指纹；密钥材料 MUST NOT 以明文返回。

创建或更新时 `credential_kind` MUST 由凭据内容判定：Service Account JSON → `google_service_account`；含 `access_key_id` 与 `secret_access_key` 的 JSON → `aws_sigv4`；否则 `api_key`。

`credential_kind = google_service_account` 时 MUST 提供合法 Service Account JSON（至少含 `type=service_account`、`client_email`、`private_key`、`token_uri`），并强制 `auth_scheme = google_service_account`。`credential_kind = aws_sigv4` 时 MUST 提供合法 AWS 凭据 JSON，并强制 `auth_scheme = sigv4`。

#### Scenario: 保存 Service Account
- **WHEN** 创建 Provider 时提供 `credential_kind = google_service_account` 与合法 JSON
- **THEN** 返回 `credential_kind` 与 `credential_masked`，且不返回密文或明文

#### Scenario: JSON 缺字段
- **WHEN** Service Account JSON 缺少 `private_key`
- **THEN** 返回 `422`、`code = invalid_request`

#### Scenario: 识别 AWS 凭据
- **WHEN** 提交含 `access_key_id` / `secret_access_key` 的 JSON 且未声明 `credential_kind`
- **THEN** 落库 `credential_kind = aws_sigv4`，`auth_scheme` 强制为 `sigv4`

#### Scenario: 内容冲突
- **WHEN** 提交 `credential_kind = api_key` 但内容为 AWS 凭据 JSON
- **THEN** 返回 `422`、`code = credential_kind_mismatch`

### Requirement: Provider 的入站 API 格式声明

系统 SHALL 在 provider key 上保存 `api_formats`（字符串数组），取值限定为 `chat_completions` / `responses` / `anthropic_messages`。空数组表示当前没有可服务的入站格式。

`gemini_generate_content` / `provider_native` 等旧取值 MUST 被视为未知取值并拒绝（`422`、`code = invalid_request`）。

预设条目的 `api_formats` MUST 取预设默认集合，请求 MUST NOT 覆盖；携带与预设不一致的值 MUST 返回 `422`、`code = preset_field_immutable`。自定义条目未提供时 MUST 默认 `["chat_completions"]`；显式提供时 MUST 去重，并拒绝未知取值（`422`、`code = invalid_request`）。

`GET /api/providers` 返回的每个预设 MUST 带默认 `api_formats` 数组，供控制台预填。

#### Scenario: 预设默认
- **WHEN** 以 `openai` 预设创建且未提供 `api_formats`
- **THEN** 保存并通过响应回显 `["chat_completions","responses"]`

#### Scenario: 兼容上游默认
- **WHEN** 以 `openai-compatible` 预设创建且未提供 `api_formats`
- **THEN** 默认 `["chat_completions"]`；可显式追加 `responses`

#### Scenario: 显式声明
- **WHEN** 自定义条目创建时提供 `api_formats = ["responses"]`
- **THEN** 保存该值并原样回显

#### Scenario: 未知取值
- **WHEN** `api_formats` 含 `openai`、`gemini_generate_content` 或 `provider_native`
- **THEN** 返回 `422`、`code = invalid_request`，不落库

#### Scenario: 预设下发格式
- **WHEN** 控制台请求 `GET /api/providers`
- **THEN** 每个预设条目都带 `api_formats`

#### Scenario: 预设锁定
- **WHEN** 以 `openai` 预设创建并提交 `api_formats = ["chat_completions"]`
- **THEN** 返回 `422`、`code = preset_field_immutable`，落库值为预设默认集合

### Requirement: Provider 模型别名映射

provider key 的 `models` MUST 为对象数组，每项包含 `name` 与可选 `upstream`。

`name` MUST 非空；`upstream` 缺失或为空时 MUST 归一化为 `name`。同一 Provider 内 `name` MUST 唯一（重复项按首次出现保留）。空白项 MUST 被丢弃。

响应（列表 / 创建 / 更新）MUST 回显归一化后的对象数组。

#### Scenario: 默认同名
- **WHEN** 提交 `models = [{"name": "gpt-5"}]`
- **THEN** 保存并回显 `[{"name": "gpt-5", "upstream": "gpt-5"}]`

#### Scenario: 别名映射
- **WHEN** 提交 `[{"name": "gpt-4o", "upstream": "gpt-4o-2024-08-06"}]`
- **THEN** 原样保存并回显该映射

#### Scenario: 去重与丢弃空名
- **WHEN** 提交 `[{"name": "gpt-5"}, {"name": " gpt-5 "}, {"name": ""}]`
- **THEN** 只保留 `[{"name": "gpt-5", "upstream": "gpt-5"}]`

#### Scenario: 旧数据迁移
- **WHEN** 数据库中存在旧的字符串数组 `["gpt-5","gpt-5-mini"]`
- **THEN** migration `0008` 将其转换为同名对象数组，`upstream` 等于 `name`

### Requirement: 对外访问地址设置

系统 SHALL 在 DB 设置中提供 `access.public_origins`（字符串数组），表示客户端访问网关使用的 origin / 路径前缀列表。

每项 MUST 为非空、绝对 `http` / `https` URL，含 host，不含 query / fragment；MUST NOT 要求以 `/v1` 结尾，已包含 `/v1` 也 MUST 接受。列表 MUST NOT 超过 10 项，MUST 去重；列表为空时，控制台回退为当前 origin。

控制台展示时 MUST 将每个 origin 规范化为 `<origin>/v1`（已以 `/v1` 结尾则保持不变）。

写入 MUST 仅允许 `admin`，通过 `PATCH /api/settings` 的 `access.public_origins`；保存后立即生效，无需重启。

`GET /api/access` MUST 对任意已认证角色返回当前列表；未认证 MUST 返回 `401`。

#### Scenario: 管理员保存多个地址
- **WHEN** admin 提交 `access.public_origins = ["https://gw.example.com","http://192.168.1.10:6565"]`
- **THEN** 保存成功并原样回显

#### Scenario: 已带 /v1
- **WHEN** admin 提交 `https://gw.example.com/v1`
- **THEN** 保存成功；控制台展示时不会重复拼接为 `/v1/v1`

#### Scenario: 普通用户读取
- **WHEN** role 为 `user` 的已认证用户请求 `GET /api/access`
- **THEN** 返回当前列表

#### Scenario: 非法地址
- **WHEN** 提交缺少 scheme、非 http(s)、含 query/fragment 或重复的地址
- **THEN** 返回 `422`，`code = invalid_request`，不写入

#### Scenario: 清空
- **WHEN** admin 提交空数组
- **THEN** 保存成功；控制台回退为 origin + `/v1`

### Requirement: 路由 ID 格式

新建路由的 `id` MUST 为 9 位数字，首位 MUST 为 1–9，其余位为 0–9；MUST 随机生成，MUST NOT 从名称、描述或时间推导，也 MUST NOT 包含字母或分隔符。

存量路由的 UUID MUST 保持不变；按原 `id` 的读取、更新、删除，以及 API Key / Provider 对 `route_id` 的关联 MUST 继续正常工作。

创建时 MUST 处理极低概率的 ID 冲突（存在性检查 + 重试）；最终失败时 MUST 返回内部错误，MUST NOT 误报为名称冲突。

#### Scenario: 新建路由
- **WHEN** 已认证用户创建路由
- **THEN** 返回的 `id` 是 9 位数字且首位非 0

#### Scenario: 存量 UUID
- **WHEN** 读取或更新一条 ID 为 UUID 的存量路由
- **THEN** 按原 ID 正常工作，不要求迁移

#### Scenario: ID 冲突
- **WHEN** 随机生成的 ID 已存在
- **THEN** 服务重新生成，最多 10 次；用户无感知

### Requirement: 路由子资源访问控制

Provider 凭据、API Key、Provider 健康等路由子资源 MUST 继承路由的 owner 作用域。

`GET/POST /api/routes/{id}/provider-keys`、`GET/PATCH/DELETE /api/provider-keys/{id}`、`GET /api/api-keys`、`GET/POST /api/routes/{id}/api-keys`、`PATCH/DELETE /api/api-keys/{id}`、`GET /api/provider-health` MUST 只允许访问当前用户拥有的路由及其子资源。`admin` MUST NOT 例外。

上列端点枚举不含 `DELETE /api/provider-health/{provider_id}`（人工解除健康标记）：该操作 MUST NOT 受本节约束，由「provider 健康人工恢复」定义（admin 全站解除，不受 owner 限制）；`GET /api/provider-health` 查询仍按 owner 隔离。

#### Scenario: 非 owner 访问 Provider
- **WHEN** 用户请求他人路由的 Provider 列表
- **THEN** 返回 `404`

#### Scenario: admin 访问他人 API Key
- **WHEN** admin 请求普通用户路由下的 API Key
- **THEN** 返回 `404`，或该 Key 不出现在列表结果中

#### Scenario: Provider 健康过滤
- **WHEN** 用户请求 `/api/provider-health`
- **THEN** 只返回自己路由下的 Provider

### Requirement: 全局用量查询（admin）

系统 SHALL 提供仅 `admin` 可访问的全站用量查询接口：

- `GET /api/admin/usage/summary`
- `GET /api/admin/usage/timeseries`
- `GET /api/admin/usage/breakdown`
- `GET /api/admin/usage/requests`

接口 MUST NOT 受 owner 作用域限制。非 admin 请求任一 `/api/admin/usage/*` MUST 返回 `403`。

#### Scenario: 全站汇总
- **WHEN** admin 请求 `/api/admin/usage/summary`
- **THEN** 返回全站请求、错误、token、延迟等汇总值

#### Scenario: 非 admin 被拒
- **WHEN** 非 admin 请求 `/api/admin/usage/summary`
- **THEN** 返回 `403`

### Requirement: 全局用量的维度与标签

全局用量接口 SHALL 支持按 `owner_id`、`route_id`、`api_key_id`、`model`、`provider_id` 过滤，并支持 `by = route | api_key | model | provider | owner` 分组。

全局统计 MUST 为只读聚合，MUST NOT 提供修改路由、Provider 或 API Key 的能力，也 MUST NOT 通过该接口返回密钥明文。

按路由分组时，若不同 owner 存在同名路由，响应 MUST 包含 owner 标识以区分。已删除路由 MUST 仍可展示历史聚合，标签为「已删除路由」；未归属路由标签为「未归属」。

#### Scenario: 按用户分组
- **WHEN** admin 请求 `breakdown?by=owner`
- **THEN** 按路由归属用户分组返回用量

#### Scenario: 按 Provider 看 token
- **WHEN** admin 请求 `breakdown?by=provider`
- **THEN** 每个 Provider 行包含输入 / 输出 token

#### Scenario: 同名路由区分
- **WHEN** 两个用户各有一条名为 `prod` 的路由
- **THEN** 全局统计按路由分组时通过 owner 区分两行

#### Scenario: 只读
- **WHEN** admin 查看全局统计
- **THEN** 不提供修改路由、Provider 或 API Key 的接口能力

### Requirement: 路由选号策略接口

每条路由记录 MUST 额外包含选号策略字段 `selection_strategy`（缺省 `round_robin`）与 `selection_config`（JSON，缺省 `{}`）；`selection_config` MUST 只保存用户显式覆盖的字段，缺省值由系统补全；既有路由 CRUD 的字段与语义不变。存量路由取缺省值即与引入本能力之前行为一致。

系统 SHALL 提供（资源名 `selection-strategy` 与列名 `selection_strategy`、领域术语「选号策略」一致）：

- `GET /api/routes/{id}/selection-strategy`：返回 `strategy`、`config`（存储的覆盖项）、`resolved`（补全默认值后的完整配置，含 `weights` / `thresholds` / `fallback`）与 `providers[]`（每个 Provider 的 `priority`、健康状态、实时指标与当前分数）；
- `PUT /api/routes/{id}/selection-strategy`：校验 → 落库 → **立即下发**数据面 → 返回与 GET 相同结构。

GET 除配置外 MUST 附带该策略的**生效快照**（`providers[]` 的指标与分数）：策略与其生效结果是同一资源的两面，拆成两个端点会出现「刚保存策略、分数还是旧的」的时序错觉。

owner 隔离与既有路由接口一致：他人路由 MUST 返回 404。

`providers[]` 的指标 MUST 取自数据面内存快照，MUST NOT 由控制面重新计算分数；无样本时 `ttfb_ms` 与 `score` 为 `null`、`stale = true`。数据面未装配时 `providers` MUST 为空数组而非报错。

#### Scenario: 读取默认策略
- **WHEN** 路由从未配置过策略
- **THEN** 返回 `strategy = "round_robin"`、`config = {}`、`resolved` 为默认权重

#### Scenario: 保存并即时生效
- **WHEN** PUT `{ "strategy": "latency" }`
- **THEN** 落库成功、数据面按新策略选号，MUST NOT 返回 `restart_required`

#### Scenario: 指标快照
- **WHEN** 某 Provider 已有样本
- **THEN** 返回其 `ttfb_ms`、`uptime_pct`、`samples`、`stale=false` 与 `score`

#### Scenario: 非 owner
- **WHEN** 非 owner 请求他人路由的选号配置
- **THEN** 返回 404

### Requirement: 选号配置校验

`PUT /api/routes/{id}/selection-strategy` MUST 校验：

| 字段 | 规则 |
|---|---|
| `strategy` | `round_robin` / `uptime` / `latency` / `auto` |
| `weights.uptime`、`weights.latency` | `[0, 1]` |
| `weights.price`、`weights.throughput` | 必须为 `0`（本版本无对应指标） |
| `thresholds.uptimePenalty` | `[1, 100]` |
| `thresholds.defaultUptime` | `(0, 100]` |
| `thresholds.defaultLatencyMs` | `[1, 600000]` |
| `thresholds.explorationRate` | `[0, 1]` |

违反 MUST 返回 `422`、`code = invalid_request` 与可读说明。`weights.uptime + weights.latency == 0` MUST NOT 被拒绝（数据面回退 `round_robin`，响应 `resolved.fallback = true`）。

#### Scenario: 非法策略
- **WHEN** PUT `{ "strategy": "price" }`
- **THEN** 返回 422 并说明支持的取值

#### Scenario: 非零价格权重
- **WHEN** PUT `weights.price = 0.3`
- **THEN** 返回 422，说明本版本不支持价格权重

#### Scenario: 全零权重允许
- **WHEN** PUT `weights.uptime = 0` 且 `weights.latency = 0`
- **THEN** 保存成功，`resolved.fallback = true`

### Requirement: Provider 优先级

provider 凭据 SHALL 具备 `priority` 字段（整数，缺省 `1`），并随数据面配置下发。`priority` 是选号档位，按降序分档。

创建与更新接口 MUST 接受 `priority`，取值 `[1, 1000]`，越界 MUST 返回 `422`。**数字越大档位越高**（`1` 为最低档，缺省值）。`0` MUST 被拒绝：暂时不参与选号 MUST 通过 Provider 状态（停用）表达。

provider 凭据的 `weight`（权重）与 `sort_order`（排序）字段 MUST NOT 存在：接口 MUST NOT 接受、DTO MUST NOT 返回，数据库列由迁移删除。

#### Scenario: 默认优先级
- **WHEN** 创建 Provider 未提供 `priority`
- **THEN** 落库为 `1`

#### Scenario: 已删除字段被忽略
- **WHEN** 请求体携带 `weight` 或 `sort_order`
- **THEN** 字段被忽略（不存在于 DTO），不影响落库

#### Scenario: 零优先级被拒绝
- **WHEN** 更新 `priority = 0`
- **THEN** 返回 422，提示改用「停用」表达；该 Provider 的档位不变

#### Scenario: 停用即退出选号
- **WHEN** 把某 Provider 状态改为停用
- **THEN** 该 Provider 立即退出选号，无需重启

#### Scenario: 越界
- **WHEN** 更新 `priority = -1` 或 `1001`
- **THEN** 返回 422

### Requirement: 故障事件记录

系统 SHALL 把 Provider 的故障记录为**事件**（`provider_incident`），而不是只保留当前状态：

- 每条记录包含：`provider_id`、`route_id`、`kind`（原因码）、`severity`、`state`（`open` / `closed`）、`started_at`、`last_seen_at`、`ended_at`、`attempts`、`last_status`、`last_error`、`resolved_by`；
- `severity` MUST 按原因码映射：`http_401` → `manual_action`；`insufficient_quota` / `http_403` → `degraded`；`http_429` / `http_5xx` / `transport_error` → `transient`；
- 同一 `provider_id + kind` 在 1 小时内再次失败 MUST 合并到已 `open` 的记录（`attempts` 累加、`last_seen_at` 前移），MUST NOT 新建；
- 凭据回池（健康事件 `Healthy`）MUST 关闭该 Provider 的全部 `open` 记录，`resolved_by = auto_probe`；
- 原因码区分 `401` 与 `403`；
- 记录 MUST NOT 依赖 `provider_keys` 外键：凭据删除后历史仍可查询。

#### Scenario: 首次故障新建记录
- **WHEN** 某 Provider 因 401 退役
- **THEN** 新建一条 `kind=http_401`、`severity=manual_action`、`state=open` 的记录

#### Scenario: 同类失败合并
- **WHEN** 同一 Provider 在 1 小时内再次因 `http_429` 被冷却
- **THEN** 已有 `open` 记录 `attempts + 1`，不新增记录

#### Scenario: 自动回池关闭
- **WHEN** 探针通过、Provider 回池
- **THEN** 其 `open` 记录全部关闭，`resolved_by=auto_probe`

#### Scenario: 配额周期性
- **WHEN** 配额耗尽进入长冷却、到期探针失败再次冷却
- **THEN** 仍为同一条记录（`attempts` 累加），不会刷出多条

### Requirement: 故障查询接口

系统 SHALL 提供：

- `GET /api/provider-incidents`：普通用户可见的范围限于自己路由下的 Provider；参数 `route_id` / `provider_id` / `state` / `since` / `limit`（≤200）；
- `GET /api/admin/provider-incidents`：仅 admin，全站范围，附带 `owner_name` / `route_name` / `provider`。

排序 MUST 为「未解决优先，其次按 `last_seen_at` 倒序」。响应 MUST 附带 Provider 名称与路由名称（缺失回退 id）。

#### Scenario: 普通用户隔离
- **WHEN** 用户请求他人路由下的故障记录
- **THEN** 返回空列表（不泄露存在性）

#### Scenario: 未解决优先
- **WHEN** 同时存在 `open` 与 `closed` 记录
- **THEN** `open` 排在前面

### Requirement: 告警 Webhook

系统 SHALL 支持通过 Webhook 推送故障告警，设置节 `alert`：

- `webhook_url`（可空，空表示不告警；仅接受 `http` / `https` 绝对 URL）
- `notify_recovery`（布尔，默认 `true`）
- `min_interval_minutes`（整数，默认 `30`）

触发规则：

- 故障记录新开且 `severity != transient` 时 MUST 发送一次；
- 记录关闭且 `notify_recovery=true` 时 MUST 发送恢复通知；
- 同一 `provider_id + kind` 在 `min_interval_minutes` 内 MUST NOT 重复发送。

发送 MUST 异步、不阻塞转发；单次请求超时 5 秒，失败最多重试 3 次（退避）；请求体 MUST NOT 包含任何凭据明文；发送结果 MUST 记录在记录的 `notified_at`。

#### Scenario: 长期故障告警
- **WHEN** 某 Provider 因 `insufficient_quota` 进入长冷却
- **THEN** 向 Webhook 发送一次 `degraded` 告警

#### Scenario: 瞬时故障不告警
- **WHEN** 某 Provider 因 5xx 短冷却
- **THEN** 不发送 Webhook

#### Scenario: 去抖
- **WHEN** 同一 Provider 的同类故障在 30 分钟内再次发生
- **THEN** 不重复发送

#### Scenario: 未配置 Webhook
- **WHEN** `webhook_url` 为空
- **THEN** 不发生任何外部请求

### Requirement: 本机地址候选接口

系统 SHALL 提供 `GET /api/network/addresses`（仅 admin），返回三组候选地址：

- `loopback`：回环地址（`127.0.0.1`、`::1`）；
- `wildcard`：通配地址（`0.0.0.0`、`::`）；
- `local`：本机网卡地址列表，元素为 `{ ip, interface, virtual_nic }`（`interface` 为网卡名；`virtual_nic` 标记 docker / 网桥 / 隧道等虚拟网卡，仅展示用）。

`local` MUST 枚举本机**全部**非回环、非 link-local、非未指定的地址（IPv4 在前），去重；枚举不可用时 MAY 回退为默认出网地址（UDP connect，不产生流量）。探测 MUST NOT 依赖外部网络请求成功；MUST NOT 阻塞请求超过 1 秒。

#### Scenario: 返回候选
- **WHEN** admin 请求该接口
- **THEN** 返回三组地址，`local` 至少包含当前出网 IP（若可探测）

#### Scenario: 非 admin
- **WHEN** 普通用户请求该接口
- **THEN** 返回 403

#### Scenario: 标记虚拟网卡
- **WHEN** 主机存在 `docker0`、`br-xxxx`、`veth*` 等虚拟网卡
- **THEN** 其地址的 `virtual_nic` 为 `true`，界面默认收起

#### Scenario: 枚举多个网卡地址
- **WHEN** 主机有 `eth0=192.168.1.10` 与 `docker0=172.17.0.1`
- **THEN** `local` 同时包含两者，并各带网卡名

#### Scenario: 探测失败
- **WHEN** 无法枚举网卡且没有默认路由
- **THEN** `local` 为空数组，接口仍返回 200

#### Scenario: 过滤不可用地址
- **WHEN** 网卡上有 169.254.x.x 或 fe80:: 地址
- **THEN** 它们不出现在 `local` 中

### Requirement: 运行期状态快照

系统 SHALL 提供单行表 `runtime_snapshot`（`id = 'current'`，`payload` 为 JSON，`updated_at` 为 Unix 秒）用于保存跨重启的**运行期状态**，当前只承载会话粘性：

~~~json
{ "affinity": [
  { "route_id": "…", "model": "…", "session": "…",
    "provider_id": "…", "expires_at": 1760000000 }
] }
~~~

- **写入**：进程收尾（优雅关闭）时 MUST 写入一次；运行期 MAY 定期写入（默认关闭，避免写放大）；
- **读取**：进程启动构造数据面配置时读取一次，过滤 `expires_at <= now` 后作为粘性种子下发；
- **上限**：条目数 MUST 有上限（如 10 000，按 `expires_at` 倒序保留）；
- **失败处理**：读失败、`payload` 非法、字段缺失 MUST 丢弃并视为空快照，MUST NOT 阻塞启动；写失败 MUST 只记录告警；
- 快照 MUST NOT 承载业务事实（用量、健康、配置的权威来源仍是各自的表）。

#### Scenario: 收尾写入
- **WHEN** 进程优雅关闭且运行期存在粘性映射
- **THEN** `runtime_snapshot` 出现一行 `id='current'`，`payload.affinity` 含未过期条目

#### Scenario: 启动恢复
- **WHEN** 进程启动且快照中有未过期条目
- **THEN** 这些条目作为粘性种子下发给数据面

#### Scenario: 过期条目不恢复
- **WHEN** 快照中的条目 `expires_at` 已过期
- **THEN** 恢复时被过滤，不下发

#### Scenario: 快照损坏不阻塞
- **WHEN** `payload` 不是合法 JSON
- **THEN** 按空快照处理，服务正常启动

### Requirement: 用量明细查询返回尝试列表

`GET /api/usage/requests` 的每一条明细 MUST 附带**每次尝试**的列表（`attempts_detail`），元素包含 `provider_id`、`status`（可为空）、`latency_ms` 与 `error`（可为空）。

明细 MUST 从**独立的明细库**（`<data_root>/usage_detail.db`）读取；路由名、归属用户与 API Key 标签由控制面用当前页出现的 id 到主库补齐，缺失时回退为 id。

- 解析失败或旧数据没有尝试记录时 MUST 返回空数组，MUST NOT 让整个查询失败；
- 列表 MUST 保持按时间倒序分页，尝试列表不额外开启请求（随行返回）；
- 明细保留口径仍由 `detail_mode` 决定（见「用量采集与持久化」）。

#### Scenario: 换号请求可回放
- **WHEN** 一次请求先 429 换号、最终 200 成功
- **THEN** 该行 `attempts_detail` 含两条：`status=429` 与 `status=200`

#### Scenario: 旧数据无尝试记录
- **WHEN** 明细行没有 `attempts_json` 或内容非法
- **THEN** 该行 `attempts_detail` 为空数组，其余字段正常返回

#### Scenario: 名称跨库补齐
- **WHEN** 明细行引用的路由与 API Key 仍存在
- **THEN** 返回它们的名称/前缀标签；已删除时回退为 id

### Requirement: 明细独立存储

逐条明细 MUST 存放于独立 SQLite 文件 `<data_root>/usage_detail.db`，与主库分离：主库 MUST NOT 再保存明细行，其旧表由迁移删除。

- 明细库 MUST 使用独立 schema（`usage_request_detail`）并至少具备 `(at)`、`(route_id, at)`、`(api_key_id, at)` 索引；
- 明细库 MUST 以 `WAL` 打开，使明细写入不阻塞主库路径（健康 / 会话 / 设置）；
- 明细库缺失或为空时 MUST 自动建表；打开失败 MUST NOT 阻塞服务启动（降级为"不记明细"并告警）；
- 保留清理 MUST 只作用于明细库，按 `detail_retention_hours` 删除。

#### Scenario: 明细与主库分离
- **WHEN** 检查 `<data_root>` 目录
- **THEN** 存在 `solidway.db` 与 `usage_detail.db` 两个文件；明细行只出现在后者

#### Scenario: 明细库打不开
- **WHEN** 明细库文件不可写
- **THEN** 服务仍正常启动与转发，仅记录告警，明细功能不可用

#### Scenario: 保留清理
- **WHEN** 明细行超过 `detail_retention_hours`
- **THEN** 被清理任务删除，聚合桶不受影响

### Requirement: 重资源生命周期

`route` 与 `user` MUST 作为重资源管理：系统 MUST NOT 通过普通删除接口物理删除它们。删除语义 MUST 为**归档**，归档 MUST 不可恢复；可逆操作只由「停用」承担。

- Route 归档后 MUST 从数据面配置中移除，MUST 释放名称，MUST 写 Provider / API Key 墓碑并删除其活行以销毁密钥材料；Route 行、用量与故障历史 MUST 保留；
- User 归档后 MUST 禁止登录、删除全部会话且不可恢复；历史归属 MUST 保留；
- 系统 MUST NOT 在普通控制台提供重资源物理清除。

#### Scenario: 删除路由等价于归档
- **WHEN** 路由 owner 请求 `DELETE /api/routes/{id}`
- **THEN** 服务归档该路由并返回 `204`；Route 行与历史保留，Provider / API Key 被删除

#### Scenario: 归档路由不再服务
- **WHEN** 某路由已归档，客户端使用该路由下的旧 API Key 发起请求
- **THEN** 数据面 MUST 不再加载该路由，请求被拒绝且 Key 永久失效

#### Scenario: 归档不可恢复
- **WHEN** owner 尝试恢复已归档路由
- **THEN** 系统 MUST NOT 提供恢复接口；需要重新提供服务时 MUST 新建路由

#### Scenario: 重复归档幂等
- **WHEN** 对已归档路由再次请求归档
- **THEN** 返回 `204`，不改变其他资源

### Requirement: Route 归档接口

系统 SHALL 提供：

- `POST /api/routes/{id}/archive`：owner 归档，不可恢复；
- `DELETE /api/routes/{id}`：等价于归档。

归档 MUST 记录 `archived_at` / `archived_by`，且 MUST NOT 提供恢复接口。

`GET /api/routes` MUST 默认只返回未归档路由；`?include_archived=true` MAY 返回归档路由。`GET /api/routes/{id}` MUST 允许 owner 只读访问归档路由。

#### Scenario: 列表默认隐藏归档
- **WHEN** owner 请求 `GET /api/routes`
- **THEN** 只返回 `archived_at IS NULL` 的路由

#### Scenario: 查看归档路由
- **WHEN** owner 请求 `GET /api/routes?include_archived=true` 或 `GET /api/routes/{id}`
- **THEN** 返回归档路由及 `archived_at` / `archived_by`

#### Scenario: 归档不可恢复
- **WHEN** owner 尝试恢复已归档路由
- **THEN** 系统 MUST NOT 提供恢复接口；需要重新提供服务时 MUST 新建路由

### Requirement: Route 归档的资源处置

归档 MUST 在同一个事务中：

- 为 Route 下的 Provider / API Key 写墓碑；
- 删除 Provider / API Key 活行，销毁密文与哈希；
- 记录 Route 的归档标记。

归档后 MUST 关闭该路由全部 `open` 故障（`resolved_by='route_archived'`），并 MUST 保留 Route 行、用量与故障历史。归档后 Route 名称 MUST 立即释放：活跃路由名称在同一 owner 内唯一，归档路由 MUST NOT 占用名称。

归档路由 MUST NOT 允许新增 / 修改 / 删除 Provider 与 API Key。

#### Scenario: 归档关闭故障
- **WHEN** 路由下存在未解决故障时归档该路由
- **THEN** 全部 `open` 故障被关闭，`resolved_by=route_archived`

#### Scenario: 归档销毁密钥
- **WHEN** 归档一条包含 Provider 与 API Key 的路由
- **THEN** 对应活行被删除，墓碑保留名称 / 前缀，MUST NOT 保留密文或哈希

#### Scenario: 归档释放名称
- **WHEN** 归档名为 `prod` 的路由后新建同名活跃路由
- **THEN** 创建成功；归档行仍以“prod（已归档）”展示

#### Scenario: 归档后禁止访问子资源
- **WHEN** owner 对归档路由请求新增或修改 Provider / API Key
- **THEN** 返回 `409`，`code` 为 `conflict`

### Requirement: Route 转移（admin）

系统 SHALL 提供 `POST /api/routes/{id}/transfer`，仅 `admin` 可调用，请求体 `{ "owner_id": "<target>" }`。

- 目标用户 MUST 存在且未归档、未停用；
- 转移 MUST 只改变 `routes.owner_id`，Route ID、Provider、API Key 与客户端调用方式 MUST NOT 改变；
- 转移 MUST NOT 追溯修改历史用量归属；
- 转移完成后 MUST 刷新用量采集器的 owner 缓存。

#### Scenario: 转移后客户端无感
- **WHEN** admin 把路由从用户 A 转移给用户 B
- **THEN** 原 API Key 继续可用；A 不再看到该路由，B 可以管理它

#### Scenario: 历史归属不变
- **WHEN** 路由在 T 时刻转移给 B，且 T 之前已产生用量
- **THEN** T 之前的用量仍归属 A，T 之后的用量归属 B

#### Scenario: 目标用户不可用
- **WHEN** 目标用户不存在、已停用或已归档
- **THEN** 返回 `422`，`code` 为 `invalid_request`

#### Scenario: 目标用户已有同名活跃路由
- **WHEN** 目标用户已有同名活跃路由
- **THEN** 返回 `409`，`code` 为 `conflict`，且不改变归属

#### Scenario: 归档路由不可转移
- **WHEN** admin 尝试转移一条已归档路由
- **THEN** 返回 `409`，`code` 为 `conflict`

### Requirement: 轻资源墓碑

Provider 与 API Key 删除（含 Route 归档时的连带删除）MUST 为物理删除，但删除前 MUST 写入 `resource_tombstone`：

- Provider 墓碑 MUST 保留名称、provider 类型、所属 Route 与 owner 快照；
- API Key 墓碑 MUST 保留名称或前缀、所属 Route 与 owner 快照；
- 墓碑 MUST NOT 保存密钥密文、哈希或明文；
- 删除 Provider MUST 关闭其全部 `open` 故障记录（`resolved_by='resource_deleted'`）。

历史查询 MUST 通过身份视图回退到墓碑标签，并返回 `deleted` 标志。

#### Scenario: 删除凭据保留历史标签
- **WHEN** 删除一个 Provider 后查询历史用量或故障
- **THEN** 名称从墓碑解析，并标记为已删除

#### Scenario: 删除 Key 后历史仍可读
- **WHEN** 删除 API Key 后查询历史用量
- **THEN** Key 标签从墓碑解析，并标记为已删除

#### Scenario: 墓碑不含秘密
- **WHEN** 检查 Provider / API Key 墓碑行
- **THEN** 不存在 `token_ciphertext`、`token_hash`、`key_hash` 或明文

### Requirement: 用量历史归属快照

`usage_bucket` 与 `usage_request_detail` MUST 保存请求发生时的 `owner_id` 快照。

- 普通统计的 owner 过滤 MUST 优先使用快照，快照为空时回退当前 `routes.owner_id`；
- Route 转移 MUST 只影响转移之后的用量；
- 用量明细补齐名称时 MUST 使用身份视图，已归档 / 已删除资源 MUST 仍可解析标签；
- 响应 MUST 携带 `archived` / `deleted` 标志，前端据此展示。

#### Scenario: 转移不改变历史
- **WHEN** 用户在 Route 转移后查询自己的历史用量
- **THEN** 转移前的请求数仍计入原 owner

#### Scenario: 归档路由历史可见
- **WHEN** owner 归档一条 Route 后查询自己的用量
- **THEN** 该 Route 的历史仍计入，并标记为已归档

### Requirement: 归档资源的用量标签

已归档 Route MUST 仍可展示历史聚合，标签 MUST 为原名称并标记“已归档”；已删除 Route MUST 回退墓碑名称，缺失时显示 `已删除路由 · <短 id>`。已删除 Provider / API Key MUST 回退墓碑标签。

#### Scenario: 归档路由区分
- **WHEN** 两条同名 Route 一条归档、一条活跃
- **THEN** 全局统计按 `route_id` 分成两行，归档行带“已归档”标记

### Requirement: 故障事件的资源存续守卫

故障事件记录 MUST 保存发生时的 `owner_id` 快照。故障写入 MUST 先校验 Provider 仍存在；Provider 已删除时 MUST NOT 新建或更新 `open` 故障。

系统 SHALL 提供 `POST /api/admin/provider-incidents/{id}/close`（仅 admin），按 incident id 关闭记录，`resolved_by='manual'`；该接口 MUST 在 Provider 已删除时仍可调用。

故障列表 MUST 通过身份视图解析 Provider / Route 名称，并返回 `archived` / `deleted` 标志。

#### Scenario: 在途事件不复活故障
- **WHEN** Provider 已删除后，其旧探针事件到达
- **THEN** 不创建新的故障记录

#### Scenario: 关闭历史孤儿故障
- **WHEN** admin 关闭一条所属 Provider 已删除的 open 故障
- **THEN** 返回 `204`，记录 `state=closed`、`resolved_by=manual`

### Requirement: Provider 端点模板与变量

每个 provider 预设 MUST 声明 `base_url_template`：一个可含 0..n 个变量的 URL 模板。预设条目写入时，`base_url` MUST 由模板与该条目的变量值**展开**得到，落库值 MUST 是展开后的绝对 `http(s)` URL。

模板 MUST 覆盖全部预设，包括端点依部署而异的条目（其模板为单变量 `${BASE_URL}`）。系统 MUST NOT 再存在"无默认 URL"这一特殊形态。

变量取值 MUST 存于该条目的 `options.base_url_vars`（对象：变量名 → 值），MUST NOT 出现在凭据字段中。

#### Scenario: 字面量模板
- **WHEN** 以 `openai` 预设创建凭据，模板为字面量、无变量
- **THEN** 落库 `base_url` 为 `https://api.openai.com/v1`，`options.base_url_vars` 为空

#### Scenario: 模板展开
- **WHEN** 以 `azure` 预设创建凭据并提交 `base_url_vars = {"resource": "myres"}`
- **THEN** 落库 `base_url` 为 `https://myres.openai.azure.com`，`options.base_url_vars.resource` 为 `myres`

#### Scenario: 全 URL 变量
- **WHEN** 以 `openai-compatible` 预设创建凭据并提交 `base_url_vars = {"BASE_URL": "https://proxy.example.com/v1"}`
- **THEN** 落库 `base_url` 为 `https://proxy.example.com/v1`

#### Scenario: 变量缺失
- **WHEN** 以 `azure` 预设创建凭据但未提供 `resource`
- **THEN** 返回 `422`，`code` 为 `missing_base_url_var`

### Requirement: 端点变量的取值约束

预设 MUST 为每个变量声明 `name`、`label`、`kind`、`required` 与可选 `pattern`，`kind` 限定为 `host-label` / `path-segment` / `host-only` / `full-url` / `gcp-location`。写入时 MUST 按 `kind` 校验：

- `host-label` / `gcp-location`：单段，不含 `.`、`/`、`?`、`#`；
- `path-segment`：不含 `/`、`?`、`#`；
- `host-only`：只允许主机名，MUST 拒绝含 `/`、`?`、`#` 的值；
- `full-url`：MUST 为绝对 `http(s)` URL，写入前去尾斜杠。

校验失败 MUST 返回 `422`，`code` 为 `invalid_base_url_var`，并指明变量名。

#### Scenario: host-only 拒绝路径
- **WHEN** 以 `databricks` 预设提交 `base_url_vars = {"host": "dbc-123.cloud.databricks.com/ai-gateway"}`
- **THEN** 返回 `422`，`code` 为 `invalid_base_url_var`

#### Scenario: full-url 去尾斜杠
- **WHEN** 以 `neon` 预设提交 `base_url_vars = {"NEON_AI_GATEWAY_BASE_URL": "https://gw.example.com/"}`
- **THEN** 落库 `base_url` 为 `https://gw.example.com/v1`

### Requirement: 上游路径模板与路径参数

预设 MAY 声明 `path_template`（上游路径骨架），用户 MUST NOT 编辑该模板。模板占位符限定为三类：`path_variables` 中声明的**路径参数**（用户按凭据填写）、`base_url_variables` 中已声明的 **URL 变量**（同名参数只填一次）、以及请求级 `{model}` / `{action}` / `{endpoint}`（MUST NOT 作为参数声明）。

未声明模板的条目 MUST 按 `{base_url}{家族后缀}` 转发。`<key>-custom` 副本 MUST 沿用所属预设的模板与参数校验；未关联预设的自定义条目 MUST 忽略模板。模板自带 query MUST 与入站 query 合并（模板在前）。

#### Scenario: Vertex 区域端点
- **WHEN** 以 `google-vertex` 预设创建凭据并提交 `base_url_vars = {"location": "us-east5"}`、`path_vars = {"project": "demo", "location": "us-east5"}`
- **THEN** `options.path_template` 为 `/v1/projects/{project}/locations/{location}/publishers/google/models/{model}:{action}`，`options.path_params` 含 `project` 与 `location`

#### Scenario: Azure 部署式路径
- **WHEN** 以 `azure` 预设创建凭据并提交 `path_vars = {"deployment": "gpt-4o-prod", "api_version": "2024-10-21"}`
- **THEN** 落库路径模板为 `/openai/deployments/{deployment}/{endpoint}?api-version={api_version}`，转发目标为 `https://{resource}.openai.azure.com/openai/deployments/gpt-4o-prod/chat/completions?api-version=2024-10-21`

#### Scenario: 路径参数缺失
- **WHEN** 以 `azure` 预设创建凭据但未提供 `deployment`
- **THEN** 返回 `422`，`code` 为 `missing_path_var`

#### Scenario: 路径参数非法
- **WHEN** 提交 `api_version = "2024/10/21"`
- **THEN** 返回 `422`，`code` 为 `invalid_path_var`

#### Scenario: 模板由服务端决定
- **WHEN** 请求在 `options` 中携带 `path_template`
- **THEN** 该值 MUST 被忽略并替换为预设模板（自定义条目则移除）

#### Scenario: 副本沿用模板
- **WHEN** 以 `google-vertex-custom` 创建凭据并显式给出 `base_url` 与 `path_vars`
- **THEN** 该条目仍带 Vertex 路径模板，转发路径按 `project` / `location` 渲染

### Requirement: 预设条目的锁定字段

当 `provider` 命中内置目录时，`base_url`、`api_formats`、`auth_scheme` MUST 由预设决定并落库，请求中 MUST NOT 覆盖。请求显式携带且与预设推导值不一致时 MUST 返回 `422`、`code` 为 `preset_field_immutable`，并在响应中指明冲突字段。

预设条目可编辑的字段 MUST 限于：端点变量值、凭据、`models`、`name`、`priority`、`status`。

#### Scenario: 覆盖端点被拒
- **WHEN** 以 `openai` 预设创建并提交 `base_url = "https://proxy.example.com/v1"`
- **THEN** 返回 `422`，`code` 为 `preset_field_immutable`，落库无记录

#### Scenario: 覆盖格式被拒
- **WHEN** 以 `google-vertex` 预设创建并提交 `api_formats = ["anthropic_messages"]`
- **THEN** 返回 `422`，`code` 为 `preset_field_immutable`

#### Scenario: 允许的字段可改
- **WHEN** 更新预设条目的 `models`、`name`、`priority`、`status` 或端点变量值
- **THEN** 更新成功，`provider` 与其余锁定字段不变

### Requirement: Provider 端点的 Google 模式

系统 MUST 以目录声明的端点模式判定"是否 Google 端点"，模式 MUST 以模板表达（如 `{prefix}.googleapis.com`），MUST NOT 在代码中硬编码域名后缀。

`credential_kind = google_service_account` MUST 仅在以下情形被接受：该条目命中的预设声明 `auth_scheme = google_service_account`，或该条目的展开后 `base_url` 命中任一 Google 端点模式。其余情形 MUST 返回 `422`、`code` 为 `service_account_requires_google_preset`。

#### Scenario: 预设允许
- **WHEN** 以 `google-vertex` 预设提交 Service Account JSON
- **THEN** 创建成功，`credential_kind` 为 `google_service_account`

#### Scenario: 自定义端点命中模式
- **WHEN** 以目录外 `provider` 提交 Service Account JSON，`base_url` 为 `https://gw.googleapis.com/v1`
- **THEN** 创建成功

#### Scenario: 自定义端点不命中
- **WHEN** 以目录外 `provider` 提交 Service Account JSON，`base_url` 为 `https://api.example.com/v1`
- **THEN** 返回 `422`，`code` 为 `service_account_requires_google_preset`

### Requirement: 预设的端点分组与变体标签

目录条目 MUST 支持 `group` 与 `variant_label`，用于在同一预设族（如 Vertex AI 的 regional / global）下分组展示。`GET /api/providers` MUST 原样返回这两个字段。

同一上游的不同端点形态 MUST 以**独立预设条目**表达，MUST NOT 以“一条预设多套模板”或“可选路径段”表达。

#### Scenario: Vertex 拆分
- **WHEN** 客户端读取目录
- **THEN** 存在 `google-vertex-anthropic` 与 `google-vertex-global-anthropic` 两条条目，`group` 均为同一 Vertex 分组，`auth_scheme` 均为 `google_service_account`

#### Scenario: 全球端点无变量
- **WHEN** 读取 `google-vertex-global-anthropic`
- **THEN** `base_url_template` 为 `https://aiplatform.googleapis.com`，`base_url_variables` 为空数组，`path_template` 含字面量 `/locations/global/publishers/anthropic/`

### Requirement: AWS SigV4 凭据

系统 SHALL 支持 `credential_kind = aws_sigv4`：凭据内容为 JSON，至少含 `access_key_id` 与 `secret_access_key`，可选 `session_token`。类型 MUST 由凭据内容判定（与 Service Account 同一机制），请求中的 `credential_kind` 仅作提示，冲突时 MUST 返回 `422`、`code = credential_kind_mismatch`。

掩码 MUST 为 `aws:<access_key_id 后 4 位>`，MUST NOT 返回明文或密文。缺少必需字段 MUST 返回 `422`、`code = invalid_request`。

`credential_kind = aws_sigv4` MUST 绑定 `auth_scheme = sigv4`；反之 `sigv4` MUST NOT 与其它凭据类型组合（`422`、`code = incompatible_auth_scheme`）。

#### Scenario: 保存 AWS 凭据
- **WHEN** 创建 Provider 时提交 `{"access_key_id":"AKIA...","secret_access_key":"..."}`
- **THEN** 返回 `credential_kind = aws_sigv4`、`credential_masked = aws:XXXX`，不返回明文

#### Scenario: 缺少字段
- **WHEN** AWS 凭据 JSON 缺少 `secret_access_key`
- **THEN** 返回 `422`、`code = invalid_request`

#### Scenario: 与鉴权方式绑定
- **WHEN** 提交 `aws_sigv4` 凭据但 `auth_scheme = bearer`
- **THEN** 返回 `422`、`code = incompatible_auth_scheme`

### Requirement: 目录的动作名与 AWS 服务声明

目录条目 MAY 声明 `path_actions`（对象，含 `unary` / `stream` 两个键），其取值 MUST 用于替换路径模板中的 `{action}`；未声明时 Anthropic 族 MUST 按 `rawPredict` / `streamRawPredict` 解析。

目录条目 MAY 声明 `aws_service`（字符串，缺省 `bedrock`），数据面 MUST 用它作为 SigV4 的服务名。目录条目 MAY 声明 `anthropic_version`（字符串），写入时 MUST 落到 `options.anthropic_version`，用于正文注入；缺省为 `vertex-2023-10-16`。

#### Scenario: Bedrock 动作名
- **WHEN** 客户端读取 `amazon-bedrock` 预设
- **THEN** 条目带 `path_template = /model/{model}:{action}`、`path_actions = {unary: "invoke", stream: "invoke-with-response-stream"}`、`aws_service = "bedrock"`、`anthropic_version = "bedrock-2023-05-31"`

#### Scenario: 缺省动作名
- **WHEN** 某 Anthropic 族预设未声明 `path_actions`
- **THEN** `{action}` 按 `rawPredict` / `streamRawPredict` 渲染

### Requirement: AWS 凭据的能力边界

`credential_kind = aws_sigv4` 的记录 MUST 不支持模型清单获取：`POST /api/models/discover` 以 `provider_key_id` 引用此类记录时 MUST 返回 `200` + 空清单 + `error_code = unsupported_credential_kind`，MUST NOT 发起上游请求；创建时的自动拉取 MUST NOT 对其触发（`autodiscover.status = "skipped"`）。

#### Scenario: AWS 凭据不支持列模型
- **WHEN** 请求以 `provider_key_id` 引用 `credential_kind = aws_sigv4` 的记录
- **THEN** 返回空清单且 `error_code = unsupported_credential_kind`，不发起上游请求

#### Scenario: 创建时不自动拉取
- **WHEN** 创建 `aws_sigv4` 凭据且模型清单为空
- **THEN** 不发起自动拉取，响应 `autodiscover.status = "skipped"`

### Requirement: 预设的兼容性分类

目录条目 MUST 只包含三种入站格式（`chat_completions` / `responses` / `anthropic_messages`）之一可服务的上游。无法映射到这三种格式的 provider（如 Gemini 原生、Amazon Bedrock 原生、GitLab Duo 原生）MUST NOT 出现在目录中。

运行期 MUST 以 `default_api_formats` 非空作为可用性判据；空格式条目 MUST 被过滤，MUST NOT 出现在 `GET /api/providers`，也 MUST NOT 被 `presets::find` 命中。

#### Scenario: 不兼容条目被剔除
- **WHEN** 维护者从 models.dev 重新生成目录
- **THEN** `@ai-sdk/google`、`@ai-sdk/google-vertex`、`@ai-sdk/amazon-bedrock`、`gitlab-ai-provider` 等无法映射到三种入站格式的条目不出现在 `providers.json`

#### Scenario: Anthropic 兼容上游被修正
- **WHEN** 条目使用 `@ai-sdk/anthropic`（如 MiniMax、Thinking Machines）
- **THEN** 其 `api_formats` 为 `["anthropic_messages"]`，而不是 `chat_completions`

#### Scenario: 空格式条目运行期过滤
- **WHEN** `providers.json` 中存在 `default_api_formats` 为空的条目
- **THEN** `GET /api/providers` 不返回该条目，按目录未命中处理

### Requirement: 预设的说明元数据

每个目录条目 MUST 带 `kind`，取值限定为 `api` / `coding_plan` / `token_plan` / `subscription` / `local` / `enterprise` / `gateway`。条目 MAY 带 `summary` 与 `note`（i18n key），已有 `doc` 继续作为文档链接；`GET /api/providers` MUST 原样返回这些字段。

面向个人订阅的编程套餐 / Token 套餐 MUST 使用 `coding_plan` / `token_plan`，并 MUST 提供“需订阅套餐、使用套餐专用 Key、普通按量 Key 通常不可用”的文案 key。特殊鉴权或协议形态 MUST 通过 `note` 给出额外说明。

#### Scenario: 元数据字段下发
- **WHEN** 客户端读取 `GET /api/providers`
- **THEN** 每个条目带 `kind`，并按需带 `summary` / `note` / `doc`

#### Scenario: Token Plan 类型
- **WHEN** 预设 key 或名称表示编程套餐 / Token 套餐（如 `zai-coding-plan`、`xiaomi-token-plan-cn`）
- **THEN** `kind` 为 `coding_plan` 或 `token_plan`，且界面可获取套餐说明文案

### Requirement: 预设的本地化名称

目录条目 MAY 带 `name_zh`（中文显示名）；`name` 仍是规范名与英文回退值。生成器 MUST 为每个保留条目写入非空 `name_zh`：专有名词可等于 `name`，描述性名称 MUST 提供中文译文。

`GET /api/providers` MUST 原样返回 `name` 与 `name_zh`。`name_zh` 属于目录数据，MUST NOT 以运行时翻译推断或拼接。

#### Scenario: 字段下发
- **WHEN** 客户端读取 `GET /api/providers`
- **THEN** 每个条目都带非空 `name` 与 `name_zh`

#### Scenario: 描述性名称有中文译文
- **WHEN** 读取 `alibaba-coding-plan` 预设
- **THEN** `name` 为 `Alibaba Coding Plan`，`name_zh` 为对应的中文名称

#### Scenario: 专有名词可同名
- **WHEN** 读取 `openai` 或 `anthropic` 预设
- **THEN** `name_zh` 可以与 `name` 相同

#### Scenario: 不兼容条目不产生名称
- **WHEN** 某个条目因协议不兼容被生成器剔除
- **THEN** 它不会出现在目录中，也不会有 `name_zh`

### Requirement: 桌面自动登录换取会话

服务 SHALL 提供 `POST /api/auth/desktop`，请求体 `{ "token": <string> }`。

服务 MUST NOT 提供任何「当前是否 desktop 模式」的公开探测接口；桌面身份只体现在令牌本身。

仅当「`desktop` 模式」+「令牌 SHA-256 与本进程签发值一致」+「admin 用户存在且可用」全部满足时，MUST 返回 `200` + `MeResponse` 并签发与 `POST /api/auth/login` 相同规格的会话 cookie。

admin 的口令哈希处于锁定态（尚未设置口令）MUST NOT 阻止换取会话：桌面会话的授权依据是令牌而非口令，此时 `MeResponse.has_password` MUST 为 `false`。

任一条件不满足时 MUST 返回 `401`，错误码 `desktop_login_unavailable`。同一令牌 MUST 允许多次换取；进程重启后 MUST 失效。接口 MUST NOT 把令牌写入日志或数据库。

#### Scenario: 换取管理员会话
- **WHEN** 桌面壳用本进程令牌调用该接口
- **THEN** 返回 `200`，携带管理员 `MeResponse` 与 `set-cookie` 会话

#### Scenario: 无口令 admin 仍可换取
- **WHEN** admin 的 `password_hash` 仍为锁定态，桌面壳用本进程令牌调用该接口
- **THEN** 返回 `200`，`MeResponse.has_password` 为 `false`

#### Scenario: 令牌复用
- **WHEN** 同一令牌在进程存活期间被调用两次
- **THEN** 两次均返回 `200`

#### Scenario: 非桌面模式
- **WHEN** 服务以 `standalone` 模式运行并调用该接口
- **THEN** 返回 `401` `desktop_login_unavailable`

#### Scenario: 无可探测的桌面状态接口
- **WHEN** 未认证客户端请求 `GET /api/auth/desktop`
- **THEN** 返回 `405`（该路径只接受 POST），不返回任何模式或凭据信息

#### Scenario: 引导未完成
- **WHEN** admin 的 `password_hash` 仍为锁定态
- **THEN** 桌面令牌可换取会话（`MeResponse.has_password` 为 `false`）；口令登录仍返回 `401`；引导接口仅 `standalone` 模式可用

#### Scenario: 令牌错误
- **WHEN** 请求携带错误或已轮换的令牌
- **THEN** 返回 `401` `desktop_login_unavailable`，不签发会话
