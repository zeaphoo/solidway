# gateway-core Specification

## Purpose
TBD - created by archiving change add-gateway-core. Update Purpose after archive.

## Requirements

### Requirement: 纯内存边界

`solidway_core` MUST NOT 执行数据库操作、读取配置文件或提供管理端点。配置 MUST 由控制面通过内存结构全量下发；运行期状态变更 MUST 通过事件流交给控制面持久化。

#### Scenario: 依赖不含数据库
- **WHEN** 检查 core 的依赖树
- **THEN** 不含 `sqlx` 或任何数据库驱动

#### Scenario: 配置由控制面下发
- **WHEN** 控制面调用 `apply_config`
- **THEN** 新配置原子生效，且 core 不读取任何文件

### Requirement: 转发入口与鉴权

网关 SHALL 以入站鉴权头中的 API Key 定位路由：把入站凭据哈希后匹配已下发的 API Key，命中则归属其 `route_id`。可用的入站头 MUST 按固定顺序尝试：`Authorization: Bearer <API Key>` → `x-api-key: <API Key>`，取**第一个**哈希命中者。两者都未命中或命中凭据已停用时 MUST 返回 401 `invalid_api_key`，且鉴权 MUST 先于端点分类。

`x-goog-api-key` MUST NOT 作为入站鉴权头；携带该头且无其它有效头时 MUST 返回 401 `invalid_api_key`。转发前 MUST 仍剥离该头，避免客户端凭据泄漏到上游。

query 中的凭据类参数（`key` / `api_key`，大小写不敏感）MUST NOT 参与鉴权：即使取值有效，缺少上述两个头时 MUST 返回 401 `invalid_api_key`。这些参数只做“不外传”处理（转发前剥离），不作为鉴权输入，避免凭据出现在 URL、访问日志与 Referer 中。

入站鉴权头 MUST NOT 按端点家族限定：任一头 MAY 用于任一端点。出站鉴权头 MUST 始终按所选 Provider 的 `auth_scheme` 重新构造，与入站头无关。

#### Scenario: 有效凭据定位路由
- **WHEN** 客户端携带某路由的 API Key 请求转发端点
- **THEN** 请求归属该 API Key 绑定的路由

#### Scenario: 无效凭据优先于端点分类
- **WHEN** 客户端携带无效凭据请求非工作端点
- **THEN** 返回 401 `invalid_api_key`，而非 404

#### Scenario: Anthropic SDK 头
- **WHEN** 客户端以 `x-api-key` 携带有效 API Key 请求 `POST /v1/messages`
- **THEN** 请求归属该 Key 绑定的路由

#### Scenario: x-goog-api-key 不再被接受
- **WHEN** 客户端仅以 `x-goog-api-key` 携带 API Key 请求任一已建模端点
- **THEN** 返回 401 `invalid_api_key`

#### Scenario: 三个头都未命中
- **WHEN** 客户端同时携带 `Authorization`、`x-api-key` 与 `x-goog-api-key` 且均无法匹配任何 API Key
- **THEN** 返回 401 `invalid_api_key`

#### Scenario: 客户端头不外传
- **WHEN** 请求以 `x-api-key` 或 `x-goog-api-key` 携带网关 API Key 被转发到上游
- **THEN** 上游收到的鉴权头为 Provider 凭据按 `auth_scheme` 构造的头，客户端 API Key 绝不外传

#### Scenario: query 凭据不参与鉴权
- **WHEN** 客户端仅在 query 中携带有效凭据（`?key=<API Key>` 或 `?api_key=<API Key>`），且不带任何入站鉴权头，请求 `POST /v1/chat/completions`
- **THEN** 返回 401 `invalid_api_key`，且 MUST NOT 触达上游

#### Scenario: 有效头与 query 凭据并存
- **WHEN** 客户端以 `Authorization: Bearer` 携带有效凭据，同时 query 含 `?key=...`
- **THEN** 鉴权按头部凭据进行，请求正常转发，且 `key` 参数 MUST 被剥离、MUST NOT 出现在上游 URL 中

### Requirement: 端点范围

网关 SHALL 只处理以下 path：

- `POST /v1/chat/completions`
- `POST /v1/responses`
- `POST /v1/messages`
- `POST /v1/messages/count_tokens`
- `GET /v1/models`
- `GET /health`（免鉴权）

其余 `/v1/*`、`/v1beta/*` 与 `/upload/*` MUST 返回 404 `unsupported_endpoint`。`/v1beta/*` 与 `/upload/*` 的路由挂载 MAY 保留，用于保证鉴权先于端点分类且不落入控制台 SPA fallback；其所有 path MUST 按未建模端点处理。任一已建模端点的方法不匹配 MUST 返回 405 并带 `Allow` 头。

`GET /v1/models` MUST 只返回**至少被一个启用中 Provider 以三种入站格式之一声明**的模型；仅有模型声明、但没有任何入站格式的 Provider MUST NOT 使其模型出现在该响应中。模型存在性判定与格式判定仍分别用于 `model_not_found` 与 `unsupported_api_format`，不因本过滤而改变。

`/v1/models` 的条目 MUST 同时包含 OpenAI list 字段（`id` / `object` / `created` / `owned_by`）与 Anthropic SDK 可识别的扩展字段（`type` / `display_name` / `created_at`），响应顶层 MUST 带 `has_more` / `first_id` / `last_id`。额外字段对 OpenAI 客户端无影响。

#### Scenario: 非工作端点拒绝
- **WHEN** 请求 `/v1/files`
- **THEN** 返回 404 `unsupported_endpoint`

#### Scenario: Gemini 路径拒绝
- **WHEN** 请求 `/v1beta/models/gemini-2.5-pro:generateContent`、`/v1beta/cachedContents` 或 `/upload/v1beta/files`
- **THEN** 返回 404 `unsupported_endpoint`（JSON，不是 HTML），不转发上游

#### Scenario: 方法不匹配
- **WHEN** 请求 `GET /v1/messages`
- **THEN** 返回 405 并带 `Allow: POST`

#### Scenario: /health 免鉴权
- **WHEN** 请求 `GET /health`
- **THEN** 网关本地应答，不转发上游

#### Scenario: /v1/models 本地合成
- **WHEN** 请求 `GET /v1/models`
- **THEN** 返回该 API Key 所属路由中至少有一种入站格式可服务的模型并集（`api_formats` 为空的 Provider 的模型 MUST NOT 出现），不转发上游

#### Scenario: 未知 Gemini 动作拒绝
- **WHEN** 请求 `/v1beta/models/gemini-2.5-pro:predict`
- **THEN** 返回 404 `unsupported_endpoint`，不转发上游

#### Scenario: Anthropic SDK 读取模型列表
- **WHEN** Anthropic SDK 以 `x-api-key` 请求 `GET /v1/models`
- **THEN** 每个条目含 `type = "model"`、`display_name` 与 `created_at`，顶层含 `has_more` / `first_id` / `last_id`

### Requirement: 保真转发与白名单改写

网关 MUST 原样转发请求体、响应体与流式事件，响应状态码原样回传；改写白名单为**五项**：

1. 鉴权头：移除入站的 `Authorization` / `x-api-key` / `x-goog-api-key` / `api-key`，替换为所选 Provider 凭据按 `auth_scheme` 构造的头；
2. 目标地址：按「入站家族与上游地址构造」拼接；
3. query 中的凭据类参数：仅移除 `key` 与 `api_key`（大小写不敏感），其余 query 顺序与字面量 MUST 原样保留；
4. 请求中承载客户端模型名的位置（仅模型别名生效时）：请求体顶层 `model`；
5. 响应中回写的模型名（仅模型别名生效时，**仅限成功响应**）：见「保真转发与白名单改写」下各家族说明与「Anthropic Messages 通路」。错误响应体（含 LastFailure 透传的上游错误体）MUST 原样透传，MUST NOT 回写 `model`。

其余字节 MUST NOT 改动；流式响应 MUST NOT 缓冲。除入站鉴权头外的请求头（含 `anthropic-version` / `anthropic-beta`）MUST 原样转发。

顶层 `model` 的改写仅发生在模型别名生效时（`upstream` 与客户端 `name` 不同）：请求把顶层 `model` 的字符串值替换为该 Provider 声明的上游模型 id；**成功响应**把顶层 `model` 回写为客户端模型名；非流式响应 MUST 整体读取后定点替换，流式响应 MUST 按事件行处理且 MUST NOT 缓冲整条流。除上述位置外，MUST NOT 改写嵌套对象或数组中的任何同名字段。

#### Scenario: 未知字段原样到达
- **WHEN** 请求体含网关不认识的字段
- **THEN** 上游收到除白名单改写点外逐字节一致的请求体

#### Scenario: 鉴权头替换
- **WHEN** 请求被路由到某 provider 凭据
- **THEN** 上游收到的鉴权头为该凭据按 `auth_scheme` 构造的头，客户端 API Key 绝不外传

#### Scenario: 家族头原样转发
- **WHEN** 客户端请求 `POST /v1/messages` 且带 `anthropic-version: 2023-06-01` 与 `anthropic-beta`
- **THEN** 上游收到同名同值的两个头

#### Scenario: query 清理
- **WHEN** 客户端请求 `/v1/chat/completions?key=<网关 Key>` 且使用有效头部鉴权（Gemini 端点已移除，query 清理规则仍适用于所有入站请求）
- **THEN** 上游 URL 不含 `key`

#### Scenario: 路由 origin 剥离
- **WHEN** 入站 path 为 `/v1/chat/completions` 且凭据 base URL 为 `https://host/v1`
- **THEN** 转发目标为 `https://host/v1/chat/completions`

#### Scenario: 嵌套同名字段不受影响
- **WHEN** 请求体顶层 `model` 被改写，且 `tools` / `messages` 等嵌套结构中存在 `model` 字段
- **THEN** 嵌套字段保持原值

#### Scenario: 流式回写
- **WHEN** 上游以 `text/event-stream` 返回，事件数据中含顶层 `model`
- **THEN** 每个 `data: {...}` 事件的顶层 `model` 被回写为客户端模型名，`data: [DONE]` 等非 JSON 行原样保留

### Requirement: 请求体上限

所有转发端点 MUST 整读请求体（换号需重发整轮请求，并需从中取 `model`），上限 32MB。超限 MUST 返回 413 `request_too_large`，读取中断 MUST 返回 400 `request_body_incomplete`。

#### Scenario: 超限
- **WHEN** 请求体超过 32MB
- **THEN** 返回 413 `request_too_large`，不转发上游

#### Scenario: 所有转发端点同受上限
- **WHEN** 任一转发端点的请求体超过 32MB（Gemini path 取模型端点已移除，规则对其余端点同样适用）
- **THEN** 返回 413 `request_too_large`

#### Scenario: resumable 上传
- **WHEN** 请求 `/upload/v1beta/files?uploadType=resumable`
- **THEN** 返回 404 `unsupported_endpoint`，不转发上游

### Requirement: 模型校验与轮换

网关 MUST 从请求体顶层 `model` 取客户端模型名：`/v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/messages/count_tokens` 均同此规则；缺失、非字符串或非合法 JSON MUST 返回 404 `model_not_found`。

provider 凭据的 `models` 清单为**必填**：清单非空且包含该 `model`（按 `name` 匹配）的凭据才可承载；**空清单不服务任何模型**。无任何凭据可承载 MUST 返回 404 `model_not_found`。

#### Scenario: 空清单不参与
- **WHEN** 路由内某凭据 `models` 为空
- **THEN** 该凭据不参与任何模型的轮换

#### Scenario: 模型不在清单内
- **WHEN** 请求 `model` 不属于路由内任何凭据的清单
- **THEN** 返回 404 `model_not_found`，不转发上游

#### Scenario: 请求体缺 model
- **WHEN** 请求 `POST /v1/messages` 且请求体不含 `model`（Gemini path 取模型端点已移除）
- **THEN** 返回 404 `model_not_found`，不转发上游

### Requirement: 选号与换号

网关 MUST 在同一路由内、**启用中、提供该模型且声明支持当前入站格式、健康可用** 的凭据中选号；候选顺序由该路由的**选号策略**决定（见「路由选号策略」）。默认策略为 `round_robin`，其行为 MUST 与引入策略之前一致。

首个字节返回前发生可换号类失败（见健康分类）时 MUST 换用下一候选凭据重发整轮请求，每个凭据对同一请求 MUST 最多尝试一次。全部失败 MUST 原样返回最后一次上游错误；首个字节之后的失败 MUST NOT 换号。

选号策略与优先级 MUST NOT 改变请求体、响应体、流式事件、状态码或鉴权头改写规则。

#### Scenario: 换号透明
- **WHEN** 首选凭据返回 429，且路由内另有健康凭据
- **THEN** 换号重发，客户端只收到最终成功响应

#### Scenario: 请求级错误不换号
- **WHEN** 上游返回 400
- **THEN** 原样返回且不换号、不标记凭据

#### Scenario: 全部失败
- **WHEN** 所有候选凭据均返回 429
- **THEN** 客户端收到最后一次 429 的原样内容

#### Scenario: 停用的凭据不参与
- **WHEN** 路由内唯一提供该模型的健康凭据处于停用状态
- **THEN** 该凭据 MUST NOT 被选中，按无可用凭据返回 `503 route_exhausted`

#### Scenario: 策略不影响字节
- **WHEN** 同一路由把策略从 `round_robin` 改为 `latency`
- **THEN** 上游收到的请求体与客户端收到的响应体逐字节不变

### Requirement: 无可用凭据

路由内无健康凭据可承载该模型时，网关 MUST 立即返回 503 `route_exhausted`，SHALL NOT 排队等待。

#### Scenario: 耗尽立即失败
- **WHEN** 该模型的所有候选凭据均处于冷却或退役
- **THEN** 返回 503 `route_exhausted`

### Requirement: 网关自造错误体

网关自造错误 MUST 按**入站 path 所属家族**选择外壳，MUST NOT 对所有家族使用同一形状：

- OpenAI 家族（`/v1` 家族的已建模端点）：`{"error":{"type","code","message"}}`；
- Anthropic 家族（`/v1/messages`、`/v1/messages/count_tokens`）：`{"type":"error","error":{"type","message","code"}}`，其中 `error.code` 为网关自有错误码。

path 不属于任何已建模端点时（如 `/v1/files`、`/v1beta/*`、`/upload/*`）MUST 使用 OpenAI 外壳。外壳选择 MUST 只依据 path 字符串，MUST NOT 依赖鉴权或端点校验结果，因此未鉴权请求也按 path 家族成形。

上游错误响应体 MUST 原样透传，不受本条约束。

#### Scenario: 错误体形状
- **WHEN** 触发任一自造错误
- **THEN** 响应体为对应家族外壳，并带对应 HTTP 状态码

#### Scenario: OpenAI 家族错误体
- **WHEN** 请求 `POST /v1/chat/completions` 且凭据无效
- **THEN** 返回 `{"error":{"type","code":"invalid_api_key","message"}}`

#### Scenario: Anthropic 家族错误体
- **WHEN** 请求 `POST /v1/messages` 且凭据无效
- **THEN** 返回 401 与 `{"type":"error","error":{"type":"authentication_error","message","code":"invalid_api_key"}}`

#### Scenario: 未建模路径的未鉴权外壳
- **WHEN** 请求 `POST /v1beta/models/gemini-2.5-pro:generateContent` 且凭据无效
- **THEN** 返回 401 与 OpenAI 外壳 `{"error":{"type","code":"invalid_api_key","message"}}`（Gemini 外壳已移除）

#### Scenario: 未建模路径
- **WHEN** 请求 `POST /v1beta/batches` 或 `POST /v1beta/models/gemini:predict`
- **THEN** 返回 OpenAI 外壳的 `unsupported_endpoint`

#### Scenario: 上游错误原样
- **WHEN** 上游返回 Anthropic 形状的 `{"type":"error","error":{...}}`
- **THEN** 客户端收到该错误体的原样字节

### Requirement: provider 健康分类

网关 MUST 把上游失败分类为凭据级标记：

- `401` → **退役**（`retired`，需人工恢复）；
- `403` → 冷却 `cooldown_auth`，原因码 `http_403`；
- `429` 且 `error.code == "insufficient_quota"` → **长冷却**（`Retry-After` 优先，否则 `quota_cooldown`，上限 7 天），原因码 `insufficient_quota`，冷却到期后按探针流程尝试回池；
- 其他 `429` → 按 `Retry-After`（缺省 `cooldown_rate_limit`）冷却，原因码 `http_429`；
- `5xx` / 网络错误 / 超时 → 冷却 `cooldown_unavailable`，原因码 `http_5xx` / `transport_error`；
- 其他 4xx → 不标记。

原因码 MUST 区分 `401` 与 `403`（不再共用同一个码）。

#### Scenario: 余额不足永久退役
- **WHEN** 上游返回 429 且 `error.code` 为 `insufficient_quota`
- **THEN** 该凭据 MUST NOT 永久退役，而是按 `quota_cooldown` 进入长冷却（本 change 的语义变更）

#### Scenario: 认证失败退役
- **WHEN** 上游返回 401
- **THEN** 该凭据进入退役状态，不再参与选号，需人工解除

#### Scenario: 封禁先冷却
- **WHEN** 上游返回 403
- **THEN** 该凭据按 `cooldown_auth` 冷却，原因码为 `http_403`

#### Scenario: 配额耗尽进入长冷却
- **WHEN** 上游返回 429 且 `error.code` 为 `insufficient_quota`，无 `Retry-After`
- **THEN** 该凭据按 `quota_cooldown` 冷却（缺省 6 小时），到期后可由探针回池

#### Scenario: 配额按 Retry-After 冷却
- **WHEN** 上游返回 429 且 `error.code` 为 `insufficient_quota` 并带 `Retry-After: 7200`
- **THEN** 冷却截止为 7200 秒后，而不是缺省值

#### Scenario: 限流按 Retry-After
- **WHEN** 上游返回 429（非配额）且带 `Retry-After: 120`
- **THEN** 冷却截止为 120 秒后

#### Scenario: 请求级错误不标记
- **WHEN** 上游返回 400
- **THEN** 凭据不被标记

### Requirement: 探测回池

冷却到期的凭据 MUST NOT 直接回池；重新参与选号之前 MUST 先通过探针（用该凭据在某格式下清单首个模型发起极简请求）。探针 MUST 按凭据声明的入站格式选择端点与载荷：`chat_completions` → `{base}/chat/completions`；`responses` → `{base}/responses`；`anthropic_messages` → `{base}/messages` 且 MUST 携带 `anthropic-version` 头。`api_formats` 为空或仅含未知取值时 MUST NOT 发探针，并保持不参与选号。

探针通过 MUST 回池；失败 MUST 按同一分类表重新处置；未通过（含不标记类失败）MUST 保持不参与选号，由后续选号惰性重探。同一凭据同时最多一个探针。

#### Scenario: 探针通过回池
- **WHEN** 冷却到期且探针成功
- **THEN** 凭据恢复健康并重新参与选号

#### Scenario: 探针失败重新冷却
- **WHEN** 探针返回 503
- **THEN** 凭据按分类重新进入冷却

#### Scenario: Anthropic 探针带版本头
- **WHEN** 对 `anthropic_messages` 凭据发探针
- **THEN** 上游收到 `POST {base}/messages` 且带 `anthropic-version`

#### Scenario: 空格式不发探针
- **WHEN** 冷却到期的凭据 `api_formats` 为空（原 `provider_native` 语义）
- **THEN** 不发探针，凭据保持不参与选号

### Requirement: 会话粘性

网关 MUST 按家族从请求中提取会话标识，并按 (路由, 模型, 会话标识) 维护**内存**粘性映射（滑动 TTL）：

- `/v1/responses`：`previous_response_id` → `conversation` → `prompt_cache_key` → `user`；
- `/v1/chat/completions`：`prompt_cache_key` → `user`；
- `/v1/messages`：请求体 `metadata.user_id`，缺失时 MUST NOT 建立粘性；
- `/v1/messages/count_tokens`：MUST NOT 建立粘性（非生成端点，无会话语义）。

承载凭据健康时 MUST 优先复用；换号成功后 MUST 更新为新凭据；换号未成功 MUST NOT 更新。

粘性映射 MAY 由控制面以种子形式注入（启动时恢复）：种子条目 MUST 带过期时刻，已过期的条目 MUST 被忽略；MUST 只在本地无该 (路由, 模型, 会话标识) 记录时采用，MUST NOT 覆盖运行期已更新的映射。

#### Scenario: 同会话复用
- **WHEN** 某会话标识的请求由凭据 A 承载且 A 健康
- **THEN** 同标识的后续请求仍优先由 A 承载

#### Scenario: 换号跟随
- **WHEN** 粘性凭据失效并换由 B 成功承载
- **THEN** 该会话后续优先由 B 承载

#### Scenario: 重启后恢复
- **WHEN** core 以包含该会话条目的种子启动，且条目未过期
- **THEN** 该会话的首个请求优先由种子里的凭据承载

#### Scenario: 过期种子被忽略
- **WHEN** 种子里的条目 `expires_at` 早于当前时刻
- **THEN** 该条目被丢弃，按无粘性处理

#### Scenario: 不覆盖运行期数据
- **WHEN** 运行期已把某会话更新为凭据 B，随后配置再次下发带有该会话的旧种子（凭据 A）
- **THEN** 仍由 B 承载

#### Scenario: Anthropic 会话键
- **WHEN** 两个 `POST /v1/messages` 请求的 `metadata.user_id` 相同
- **THEN** 后一个请求优先复用前一个的承接 Provider

### Requirement: 事件流

core MUST 通过事件流向控制面输出：

- 每次逻辑请求的 `Access`：路由 / API Key / 客户端 model / 最终承接 provider / API 格式 / 状态 / 尝试次数 / TTFB / 请求体字节；
- 每次逻辑请求的 `Completion`：完成 / 中断 / 上游流错误、总耗时、响应体字节、token 用量（可能缺失）；
- 每次凭据尝试的 `Attempt`；
- 每次健康状态迁移的 `HealthChanged`。

无订阅者时事件 MUST 被丢弃，SHALL NOT 阻塞转发。

`Access` MUST 在选号完成并拿到上游响应头后发出；`Completion` MUST 在响应体正常结束、客户端中断或上游流错误时发出。`Completion` 的 `provider_id` MUST 与最终承接请求的 Provider 一致；全部失败时为空。

#### Scenario: 健康变更外发
- **WHEN** 某凭据被标记或探测回池
- **THEN** 发出 `HealthChanged` 事件

#### Scenario: 访问记录外发
- **WHEN** 一次转发完成
- **THEN** 发出 `Access` 事件（含尝试次数、TTFB 与字节数）

#### Scenario: 请求完成
- **WHEN** 一次逻辑请求完成
- **THEN** 发出 `Access` 与 `Completion` 事件，`Completion` 包含 token（若上游提供）与响应体字节数

#### Scenario: 换号后完成
- **WHEN** 请求先尝试 Provider A 失败，再由 Provider B 成功
- **THEN** `Completion` 的 `provider_id` 为 B，token 归属于 B

#### Scenario: 客户端中断
- **WHEN** 流式响应中途客户端断开
- **THEN** 发出 `Completion`，outcome 为 `aborted`

#### Scenario: 无订阅者
- **WHEN** 控制面没有订阅事件
- **THEN** 事件被丢弃，转发不受影响

#### Scenario: 健康变更
- **WHEN** 某 provider 凭据健康状态迁移
- **THEN** 发出 `HealthChanged` 事件

### Requirement: API Key 到期校验

网关 MUST 在每次请求鉴权时检查 API Key 的 `expires_at`；当前时间大于等于到期时间时 MUST 视为无效凭据，返回 `401`、`code = invalid_api_key`，且 MUST NOT 访问上游。

`expires_at` 为空（NULL / 未下发）MUST 视为永不过期。到期后 MUST NOT 依赖配置重新下发才生效。

#### Scenario: 已过期
- **WHEN** 请求携带的 API Key 已过 `expires_at`
- **THEN** 返回 `401 invalid_api_key`，不访问上游

#### Scenario: 未到期
- **WHEN** `expires_at` 在未来
- **THEN** 正常鉴权与转发

#### Scenario: 永不过期
- **WHEN** API Key 没有 `expires_at`
- **THEN** 行为与改动前一致

### Requirement: 按入站 API 格式选择 Provider

网关 SHALL 只把请求转发给声明支持该入站 API 格式的 Provider；SHALL NOT 做任何请求 / 响应格式转换，也 SHALL NOT 改写请求体。

每个 Provider MUST 携带 `api_formats` 集合，取值限定为 `chat_completions` / `responses` / `anthropic_messages`；空集合表示当前没有可服务的入站格式。

选号 MUST 为：模型命中 ∩ 格式命中 ∩ 健康可用。会话粘性命中的 Provider 若格式不匹配，MUST 被跳过并按格式候选顺序继续。

模型存在、但路由内没有任何 Provider 支持当前入站格式时，网关 MUST 返回 `404`、`code = unsupported_api_format`；MUST NOT 尝试上游，MUST NOT 标记任何 Provider 健康状态。

#### Scenario: 格式命中
- **WHEN** 入站 `POST /v1/responses` 且某 Provider 的 `api_formats` 含 `responses`
- **THEN** 该 Provider 可成为候选

#### Scenario: 格式不匹配
- **WHEN** 路由内 Provider 只声明 `chat_completions`
- **THEN** 它们不参与 `/v1/responses` 的候选

#### Scenario: 无 Provider 支持该格式
- **WHEN** 模型存在但无 Provider 声明 `responses`
- **THEN** 返回 `404`、`code = unsupported_api_format`，不访问上游

#### Scenario: 粘性跨格式
- **WHEN** 粘性命中的 Provider 不支持当前入站格式
- **THEN** 跳过该 Provider，按格式候选顺序继续

#### Scenario: 不做转换
- **WHEN** 入站格式与所有候选 Provider 的声明都不匹配
- **THEN** 网关 MUST NOT 转换请求体，按「无 Provider 支持该格式」返回错误

### Requirement: 模型别名映射

Provider 的模型清单 MUST 为 `{name, upstream}` 对象数组。`name` 是客户端可见模型名，用于路由、`/v1/models` 与会话粘性；`upstream` 是上游模型 id，为空时等同于 `name`。

清单按 `name` 精确匹配请求的 `model`；同一 `name` 可被多个 Provider 声明并映射到不同 `upstream`。响应中的顶层 `model` MUST 回写为 `name`。

#### Scenario: 别名命中
- **WHEN** 客户端请求 `model = gpt-4o`，某 Provider 声明 `{name: "gpt-4o", upstream: "gpt-4o-2024-08-06"}`
- **THEN** 路由命中，上游收到 `model = gpt-4o-2024-08-06`

#### Scenario: 同名不改编排
- **WHEN** `upstream` 为空或等于 `name`
- **THEN** 请求体与响应体不做任何 `model` 改写

### Requirement: 用量元数据提取

网关 SHALL 在**不改变线上响应字节**的前提下提取用量元数据：

- 非流式响应：从 JSON 响应中提取 `usage`；
- 流式响应：从 SSE 事件中提取 usage，支持 OpenAI Chat 的 usage chunk、OpenAI Responses 的 `response.completed`、Anthropic 的 `message_start` / `message_delta`；
- 统计 `bytes_out`；
- 无法找到 usage 时 MUST 在完成事件中标记为缺失，MUST NOT 估算 token。

提取过程 MUST NOT 修改响应字节、MUST NOT 注入 `stream_options` 等请求参数；流式观察 MUST NOT 改变事件内容，也 SHALL NOT 因统计而无限缓冲响应。

#### Scenario: 非流式 usage
- **WHEN** 上游 JSON 响应包含 `usage`
- **THEN** 完成事件携带归一化后的 input / output token

#### Scenario: 流式 usage
- **WHEN** 流式响应最后包含 usage（或 `response.completed`）
- **THEN** 完成事件携带对应 token，且转发字节不变

#### Scenario: usage 缺失
- **WHEN** 上游响应没有 usage
- **THEN** 完成事件标记 usage 缺失，token 为 0，不进行估算

#### Scenario: 不修改请求
- **WHEN** 客户端未请求流式 usage
- **THEN** 网关 MUST NOT 自动注入 `stream_options.include_usage`

### Requirement: 路由选号策略

每条路由 MUST 携带一个选号策略，取值为 `round_robin` / `uptime` / `latency` / `auto`；缺省 MUST 为 `round_robin`。策略由控制面随配置下发，变更 MUST 原子生效，MUST NOT 要求重启。

| 策略 | 排序依据 |
|---|---|
| `round_robin` | 该 (路由, 模型) 的轮询计数器顺序；MUST NOT 打分 |
| `uptime` | 档位分组后，档内按 可用性优先权重 + 指数 uptime 惩罚 排序 |
| `latency` | 档位分组后，档内按 首字节时延优先权重 + 指数 uptime 惩罚 排序 |
| `auto` | 档位分组后，档内按 两项权重加权 + 指数 uptime 惩罚 排序，并按 `explorationRate` 随机探索 |

**档位分组**：`priority` MUST NOT 以任何形式进入打分（不作惩罚项、不作权重）；候选 MUST 先按 `priority` 降序分档，档内再按上表排序依据排列（与「Provider 优先级（档位）」一致）。

任一策略下，命中会话粘性且健康可用的凭据 MUST 置顶；粘性命中但不可用（不健康、格式不符、`priority = 0`）时 MUST 跳过并继续按策略顺序尝试。

策略为 `round_robin` 时 `explorationRate` MUST NOT 生效（轮询本身已分散流量）。

#### Scenario: 轮询保持默认行为
- **WHEN** 路由未显式配置策略
- **THEN** 按轮询计数器顺序选号，与引入本能力之前一致

#### Scenario: 时延优先
- **WHEN** 策略为 `latency`，两个健康凭据的 EWMA 首字节时延分别为 200ms 与 900ms
- **THEN** 先尝试 200ms 的凭据

#### Scenario: 可用性优先
- **WHEN** 策略为 `uptime`，两个健康凭据的 EWMA 失败率分别为 0 与 0.3
- **THEN** 先尝试失败率为 0 的凭据

#### Scenario: 粘性优先于策略
- **WHEN** 策略为 `latency` 且会话粘性命中一个健康凭据（时延不是最低）
- **THEN** 先尝试该粘性凭据

#### Scenario: 粘性不可用时跳过
- **WHEN** 粘性命中的凭据 `priority = 0` 或不支持当前入站格式
- **THEN** 跳过该凭据，按策略顺序继续

### Requirement: Provider 优先级（档位）

每个 Provider 凭据 MUST 携带 `priority`（整数，合法范围 `1..=1000`，缺省 `1`）。`priority` 是**档位**，不是分数的一部分：候选先按 `priority` **降序分档**，档内再按策略（轮询或打分）排列；档与档之间不做加权，高档位 MUST 全部排在低档位之前。

「暂时不参与选号」MUST 通过 Provider 的启用状态（停用）表达，MUST NOT 用 `priority` 表达；`0` 不是合法档位。

`priority` MUST NOT 以任何形式加入打分（不得作为惩罚项或权重）。档内同分（或轮询起点）时按该 (路由, 模型) 的轮询计数器打散。

Provider 的 `weight`（权重）与 `sort_order`（排序）字段 MUST NOT 存在：顺序与偏好只由 `priority` 表达。

#### Scenario: 高档位绝对优先
- **WHEN** 策略为 `latency`，`priority = 2` 的凭据时延 900ms、`priority = 1` 的凭据时延 100ms
- **THEN** 先尝试 `priority = 2` 的凭据

#### Scenario: 同档内按策略
- **WHEN** 两个凭据 `priority` 相同，策略为 `latency`
- **THEN** 先尝试时延更低者

#### Scenario: 优先级不覆盖健康判定
- **WHEN** 某凭据 `priority` 很高但处于冷却或不健康
- **THEN** 该凭据 MUST NOT 进入候选

#### Scenario: 停用的凭据不参与
- **WHEN** 某凭据状态为停用
- **THEN** 它不出现在任何策略的候选中，与档位数值无关

### Requirement: 选号指标

数据面 MUST 为每个 Provider 凭据在**内存**中维护选号指标：首字节时延 EWMA 与失败率 EWMA。指标 MUST NOT 持久化、MUST NOT 读取数据库或配置文件。

- 成功拿到上游响应头时 MUST 更新首字节时延与失败率（失败率向 0 收敛）；
- 标记类失败（凭据级 401/403/429/5xx/传输失败）MUST 更新失败率（向 1 收敛），MUST NOT 更新首字节时延；
- 请求级错误（不标记凭据的 4xx）MUST NOT 计入任何指标；
- 缺失指标（无样本，或距最后一次样本超过 30 分钟）MUST 使用默认值：`defaultUptime` 与 `defaultLatencyMs`；
- 同一 Provider 的指标 MUST 在路由之间共享（凭据是全局健康判定的一部分），进程重启后清零。

#### Scenario: 成功样本更新时延
- **WHEN** 某凭据成功返回首字节
- **THEN** 该凭据的时延 EWMA 与失败率被更新，样本数 +1

#### Scenario: 失败样本不影响时延
- **WHEN** 某凭据返回 503
- **THEN** 失败率上升，时延 EWMA 保持不变

#### Scenario: 请求级错误不计入
- **WHEN** 某凭据返回不标记的 400
- **THEN** 该凭据的指标不发生任何变化

#### Scenario: 陈旧指标
- **WHEN** 某凭据最后一次样本在 31 分钟前
- **THEN** 打分使用默认值，并把它标记为陈旧

### Requirement: 打分公式

非 `round_robin` 策略 MUST 使用同一打分公式（越小越好）：

~~~txt
score = (w_uptime/Σw)·uptimeRatio + (w_latency/Σw)·latencyRatio + uptimePenalty

uptimeRatio   = maxUptime / uptime − 1
latencyRatio  = ttfb / minTtfb − 1
uptimePenalty = uptime < uptimePenaltyThreshold
              ? ((uptimePenaltyThreshold − uptime)/uptimePenaltyThreshold × 5)²
              : 0
~~~

各项含义：`uptime = (1 − failureRate) × 100`；`minTtfb` / `maxUptime` 取当前候选集合内的极值；缺失指标用默认值代入。

默认权重与阈值：`uptime` 策略 `w_uptime=0.9 / w_latency=0.1`；`latency` 策略 `w_uptime=0.1 / w_latency=0.9`；`auto` 策略 `w_uptime=0.75 / w_latency=0.25`。`w_price` 与 `w_throughput` 本轮 MUST 恒为 0（本版本无价格与吞吐指标）。

#### Scenario: 指数惩罚生效
- **WHEN** 某候选 uptime = 80%，另一候选 99%
- **THEN** 前者额外承担约 0.62 的惩罚项，可被后者反超

#### Scenario: 分数不含档位
- **WHEN** 两个候选指标相同但 `priority` 不同
- **THEN** 二者分数相同，顺序由档位决定（见「Provider 优先级（档位）」）

### Requirement: 打分回退与平局打散

权重和 `Σw ≤ 0` 时 MUST 回退 `round_robin`，MUST NOT 报错。分数相同（或候选全部无样本）时 MUST 按该 (路由, 模型) 的轮询计数器打散，保证公平与探索。

`auto` 策略的 `explorationRate`（缺省 `0.01`）MUST 按比例随机跳过打分、改用轮询顺序，为冷启动提供样本；`round_robin` / `uptime` / `latency` 策略下该值 MUST NOT 生效。

阈值缺省：`uptimePenaltyThreshold=95`、`defaultUptime=100`、`defaultLatencyMs=1000`。

#### Scenario: 权重和为零回退轮询
- **WHEN** `auto` 策略的两项权重都被配置为 0
- **THEN** 按 `round_robin` 选号，不返回错误

#### Scenario: 同分打散
- **WHEN** 两个候选分数相同（或都无样本）
- **THEN** 按轮询计数器顺序决定先试谁

#### Scenario: 探索取样
- **WHEN** `auto` 策略的 `explorationRate = 1`
- **THEN** 每次请求都按轮询顺序选号（不按打分）

### Requirement: 选号策略的即时生效

`apply_config` 替换配置时，选号策略、权重、阈值与 `priority` MUST 一并生效，MUST NOT 要求重启、MUST NOT 清空已积累的选号指标；被删除的 Provider 的指标 MUST 被丢弃。

#### Scenario: 策略即时切换
- **WHEN** 控制面把某路由策略从 `round_robin` 改为 `latency`
- **THEN** 下一次请求即按新策略选号

#### Scenario: 指标保留
- **WHEN** 只修改权重并重新下发配置
- **THEN** 已积累的指标与样本数保持不变

### Requirement: 启动时恢复运行期状态

core MUST 支持在启动或配置下发时接收运行期状态种子，且种子 MUST 只影响排序与粘性，MUST NOT 改变请求体、响应体、流式事件与状态码。

种子分两类：

- **选号指标种子**：每个 provider 的首字节时延均值、失败率与样本数；
- **粘性种子**：见「会话粘性」。

采用规则：

1. 选号指标种子 MUST 只在本地该 provider **无样本**（`samples == 0`）时采用，MUST NOT 覆盖运行期已积累的指标；
2. 粘性种子 MUST 只在本地无该会话记录时采用；
3. 种子缺失、字段非法或过期 MUST 被静默忽略，MUST NOT 阻塞启动、MUST NOT 报错给客户端；
4. 种子 MUST NOT 影响「provider 健康分类」的结果（健康由健康种子与运行期失败独立决定）。

#### Scenario: 指标种子只在无样本时生效
- **WHEN** core 启动时收到某 provider 的指标种子（时延 300ms、失败率 0、样本 42）
- **THEN** 该 provider 的初始指标即为种子值，`samples = 42`

#### Scenario: 运行期数据优先
- **WHEN** 运行期已记录该 provider 的样本，随后配置再次下发种子
- **THEN** 指标保持不变，种子被忽略

#### Scenario: 种子非法不阻塞启动
- **WHEN** 种子带有负数样本或非有限时延
- **THEN** 该条种子被忽略，core 正常启动并按未知指标（默认值）处理

### Requirement: 导出运行期粘性

core MUST 提供只读的运行期状态导出接口，供控制面在进程收尾时落盘：返回当前**未过期**的粘性映射条目（路由 / 模型 / 会话标识 / provider / 过期时刻）。

导出 MUST NOT 阻塞转发路径，MUST NOT 修改内存中的映射；条目数超过上限时 MAY 按过期时刻截断。

#### Scenario: 收尾导出
- **WHEN** 进程收到关闭信号后调用导出
- **THEN** 返回未过期的粘性条目，随后进程才能退出

#### Scenario: 空映射
- **WHEN** 运行期没有任何粘性记录
- **THEN** 导出返回空列表（不是错误）

### Requirement: 自造错误码与家族字段映射

网关自有错误码到各家族字段的映射 MUST 按下表，HTTP 状态码对所有家族一致：

| 网关 code | HTTP | OpenAI `error.code` | Anthropic `error.type` |
|---|---|---|---|
| `invalid_api_key` | 401 | `invalid_api_key` | `authentication_error` |
| `unsupported_endpoint` | 404 | `unsupported_endpoint` | `not_found_error` |
| `route_not_found` | 404 | `route_not_found` | `not_found_error` |
| `model_not_found` | 404 | `model_not_found` | `not_found_error` |
| `unsupported_api_format` | 404 | `unsupported_api_format` | `not_found_error` |
| `request_too_large` | 413 | `request_too_large` | `request_too_large` |
| `request_body_incomplete` | 400 | `request_body_incomplete` | `invalid_request_error` |
| `route_exhausted` | 503 | `route_exhausted` | `api_error` |

映射 MUST 是纯查表，MUST NOT 依据上游响应内容推断。

#### Scenario: 映射一致
- **WHEN** 任一网关 code 在两个家族外壳中返回
- **THEN** HTTP 状态码相同，家族字段按下表取值

#### Scenario: 耗尽
- **WHEN** 路由内无健康凭据，Anthropic 家族请求失败
- **THEN** 返回 503 与 `error.type = "api_error"`

### Requirement: 入站家族分类

网关 SHALL 把每个入站端点归类到一个 API 家族，并绑定家族根：

- `chat_completions`，家族根 `/v1`：`POST /v1/chat/completions`；
- `responses`，家族根 `/v1`：`POST /v1/responses`；
- `anthropic_messages`，家族根 `/v1`：`POST /v1/messages`、`POST /v1/messages/count_tokens`。

家族与家族根 MUST 只按上表取值。未在上表出现的家族 MUST NOT 服务任何入站请求。模型名来源见「模型校验与轮换」。

#### Scenario: Anthropic 从请求体取模型名
- **WHEN** 入站 `POST /v1/messages`
- **THEN** 客户端模型名取自请求体顶层 `model`

#### Scenario: OpenAI 从请求体取模型名
- **WHEN** 入站 `POST /v1/responses`
- **THEN** 客户端模型名取自请求体顶层 `model`

#### Scenario: 家族元数据不参与推断
- **WHEN** 某凭据 `auth_scheme = sigv4`，但 `api_formats` 含 `anthropic_messages`
- **THEN** 家族与家族根仍由入站端点决定，`auth_scheme` 不参与推断

### Requirement: 上游地址构造

上游目标地址 MUST 为：`{base_url}`（去尾斜杠，经版本段归一化）+ 入站 path 去掉**该家族的家族根** + 原样 query（扣除凭据类参数）。

`base_url` MUST 被视为上游 API 根，且 MUST 是**已展开**的绝对地址：控制面在写入时已按预设模板与变量值展开，数据面 MUST NOT 再做模板替换，也 MUST NOT 处理 `{...}` 或 `${...}` 占位符。

版本段归一化 MUST 按家族执行，且 MUST 只作用于转发目标的构造：

- **版本段判定**：去尾斜杠后的 `base_url` path 中存在名为 `v1` / `v1beta` / `v1alpha` 的 path 段（**段名精确比对**，含路径尾部；`v1beta1`、`v1alpha2` 等相邻形态不算命中）时视为**已含版本段**，MUST 原样使用；
- `anthropic_messages` 家族：未含版本段时 MUST 补 `/v1`（覆盖各家公布的 anthropic `base_url` 不含 `/v1` 的惯例，客户端照抄厂商文档值即可用）；
- `chat_completions`、`responses` 家族：MUST NOT 补任何版本段（`base_url` 仍是完整 API 根，含其任意兼容前缀）；
- 归一化 MUST NOT 回写存储的 `base_url`，也 MUST NOT 改写用户输入；
- 家族根 MUST 只剥离一次，MUST NOT 二次剥离。

当凭据声明了 `path_template` 时，目标地址 MUST 为已展开的 `base_url` + 按模板渲染出的路径；`base_url` MUST 已含区域主机（如 `https://us-east5-aiplatform.googleapis.com`），数据面 MUST NOT 再替换其中的 `{location}` / `{region}` / `{project}`。

#### Scenario: Anthropic 目标地址
- **WHEN** 入站 `POST /v1/messages`，凭据 `base_url = https://api.anthropic.com/v1`
- **THEN** 转发目标为 `https://api.anthropic.com/v1/messages`

#### Scenario: Anthropic 缺版本段自动补齐
- **WHEN** 入站 `POST /v1/messages`，凭据 `base_url = https://api.example.com/apps/anthropic`
- **THEN** 转发目标为 `https://api.example.com/apps/anthropic/v1/messages`

#### Scenario: Anthropic 缺版本段补齐（count_tokens）
- **WHEN** 入站 `POST /v1/messages/count_tokens`，凭据 `base_url = https://api.deepseek.com/anthropic`
- **THEN** 转发目标为 `https://api.deepseek.com/anthropic/v1/messages/count_tokens`

#### Scenario: Anthropic 私有前缀
- **WHEN** 入站 `POST /v1/messages`，凭据 `base_url = https://proxy.example.com/anthropic/v1`
- **THEN** 转发目标为 `https://proxy.example.com/anthropic/v1/messages`

#### Scenario: 不重复添加版本段
- **WHEN** 凭据 `base_url` 已含 `/v1` 且入站为 `/v1/messages`
- **THEN** 目标地址中 `/v1` 只出现一次

#### Scenario: 家族根只剥离一次
- **WHEN** 入站 `POST /v1/messages/count_tokens` 且 `base_url` 以 `/v1` 结尾
- **THEN** 目标 path 为 `/v1/messages/count_tokens`

#### Scenario: OpenAI 兼容上游前缀保持可用
- **WHEN** 入站 `POST /v1/chat/completions`，凭据 `base_url = https://open.bigmodel.cn/api/paas/v4`
- **THEN** 转发目标为 `https://open.bigmodel.cn/api/paas/v4/chat/completions`

#### Scenario: OpenAI 兼容族不补版本段
- **WHEN** 入站 `POST /v1/chat/completions`，凭据 `base_url = https://api.deepseek.com`
- **THEN** 转发目标为 `https://api.deepseek.com/chat/completions`，MUST NOT 补 `/v1`

#### Scenario: 归一化不回写 base_url
- **WHEN** 用户保存 `api_formats = ["anthropic_messages"]` 且 `base_url = https://api.example.com/apps/anthropic`
- **THEN** 落库与界面展示的 `base_url` 仍为用户输入值，补齐只发生在转发目标构造

#### Scenario: Vertex 区域主机已展开
- **WHEN** 凭据带 Vertex Anthropic 路径模板、`base_url = https://us-east5-aiplatform.googleapis.com`、`options = {project: "proj", location: "us-east5"}`
- **THEN** 目标地址为 `https://us-east5-aiplatform.googleapis.com/v1/projects/proj/locations/us-east5/publishers/anthropic/models/...`，主机部分不被再次改写

#### Scenario: Vertex 全球端点
- **WHEN** 凭据带全球端点路径模板、`base_url = https://aiplatform.googleapis.com`、`options = {project: "proj", location: "global"}`
- **THEN** 目标路径使用 `locations/global`

#### Scenario: 不做运行期模板替换
- **WHEN** 凭据 `base_url` 含未展开的 `{` 或 `$` 占位符
- **THEN** 数据面 MUST NOT 尝试替换，请求按地址不可达失败；此类记录由控制面的写入校验阻止

### Requirement: Anthropic Messages 通路

网关 SHALL 支持 `anthropic_messages` 家族的两个端点：`POST /v1/messages` 与 `POST /v1/messages/count_tokens`。请求体与响应体 MUST 逐字节透传（除白名单改写点）。

模型别名生效时：请求 MUST 把顶层 `model` 替换为上游模型 id；非流式响应 MUST 把顶层 `model` 回写为客户端模型名；流式响应 MUST 只回写 `message_start` 事件中的 `message.model`，其余行（`event:` 行、其他 `data:` 事件、`[DONE]`）MUST 原样保留且 MUST NOT 缓冲整条流。

`/v1/messages/count_tokens` 是**非生成端点**：其成功响应 MUST NOT 被改写；请求 MUST 计入请求数，但 MUST NOT 计入 token 用量（响应不含官方 usage）、MUST NOT 计入「用量缺失」计数（该计数只表示生成请求未拿到官方 usage），MUST NOT 估算。

#### Scenario: 保真转发
- **WHEN** 客户端请求 `POST /v1/messages` 且体含 `system` / `tools` / `thinking` 等网关不认识的字段
- **THEN** 上游收到逐字节一致的请求体

#### Scenario: 非流式别名回写
- **WHEN** 别名生效且上游返回顶层 `model = claude-sonnet-4-5-20250929`
- **THEN** 客户端收到 `model` 为客户端模型名，其余字节不变

#### Scenario: 流式别名回写点
- **WHEN** 别名生效且上游返回 `message_start` 事件，其 `message.model` 为上游 id
- **THEN** 该事件中 `message.model` 被回写为客户端模型名，其他事件与行原样保留

#### Scenario: count_tokens 转发
- **WHEN** 客户端请求 `POST /v1/messages/count_tokens`
- **THEN** 上游响应原样回传，计入请求数

#### Scenario: count_tokens 不计 token
- **WHEN** `POST /v1/messages/count_tokens` 成功返回 `{"input_tokens": 1024}`
- **THEN** token 用量为 0，且不计入「用量缺失」

### Requirement: 目标地址构造方式

数据面 MUST 按凭据上的 `path_template` 渲染上游路径：路径参数来自 `options.path_params` / `options.base_url_vars`，`{model}` 用别名后的上游模型 id，`{endpoint}` 用入站 path 去家族根后的剩余段。`{action}` MUST 优先取预设声明的 `path_actions`（`unary` / `stream`）；未声明时 Anthropic 族按 `rawPredict` / `streamRawPredict` 解析。模板自带 query MUST 与入站 query 合并（模板在前）。未声明模板时 MUST 按 `{base_url}{家族后缀}` 转发。

当 Anthropic 入站且路径模板把模型放在路径中（含 `{model}`）时，请求体 MUST 去掉顶层 `model` 并注入 `anthropic_version`；其余情况正文保真转发。模板与参数 MUST 由控制面冻结下发，数据面 MUST NOT 依据 `provider` 名称前缀或目录内容推断构造方式。

#### Scenario: 按字段选构造方式
- **WHEN** 凭据下发时带 `path_template` 且 `{action}` 解析为 rawPredict
- **THEN** 该凭据的 Anthropic 入站请求 MUST 按模板渲染路径

#### Scenario: 标识改名不影响构造方式
- **WHEN** 带 `path_template` 的凭据其 `provider` 被改为任意短名
- **THEN** 转发仍按模板渲染路径

#### Scenario: 非 Vertex 协议
- **WHEN** 凭据未声明 `path_template`
- **THEN** 请求按家族拼接端点路径，MUST NOT 触发模板渲染

#### Scenario: 预设声明的动作名
- **WHEN** 预设声明 `path_actions = {unary: "invoke", stream: "invoke-with-response-stream"}` 且入站为 Anthropic 流式
- **THEN** `{action}` 渲染为 `invoke-with-response-stream`

### Requirement: SigV4 请求签名

当凭据的 `auth_scheme = sigv4` 时，数据面 MUST 对每个转发请求按 AWS Signature Version 4 签名；签名 MUST 基于**实际发送**的 method / path / query / headers / body，探针 MUST 复用同一流程。

#### Scenario: 请求被签名
- **WHEN** 凭据 `auth_scheme = sigv4`、`options.region = "us-east-1"`，转发 `POST /v1/messages`
- **THEN** 上游收到带 `AWS4-HMAC-SHA256 Credential=.../us-east-1/bedrock/aws4_request` 的 `Authorization`，signed headers 含 `host;x-amz-date`

#### Scenario: 签名覆盖改写后的请求体
- **WHEN** 请求体经别名改写或 rawPredict 形状改写后发送
- **THEN** payload hash 对应改写后的最终字节，MUST NOT 对应原始字节

#### Scenario: 探针签名
- **WHEN** 对 `sigv4` 凭据发起探针
- **THEN** 探针与真实转发使用同一模板渲染与签名流程

### Requirement: SigV4 签名要素

签名 MUST 使用 `host` 与 `x-amz-date` 作为 signed headers；凭据含 `session_token` MUST 时额外发送 `x-amz-security-token` 并纳入签名。payload hash MUST 为实际请求体的 SHA-256（小写十六进制）。

credential scope MUST 为 `<yyyymmdd>/<region>/<aws_service>/aws4_request`：`region` 取凭据 `options.region`，缺省回退 `options.base_url_vars.region`；`aws_service` 取预设声明（缺省 `bedrock`）。时间 MUST 使用 UTC。

#### Scenario: 会话令牌入签名
- **WHEN** 凭据含 `session_token`
- **THEN** 请求带 `x-amz-security-token`，且该头出现在 signed headers 中

#### Scenario: 区域与服务来自凭据
- **WHEN** `options.region = "us-west-2"` 且预设声明 `aws_service = "bedrock"`
- **THEN** scope 为 `<date>/us-west-2/bedrock/aws4_request`

#### Scenario: 时钟偏差可读
- **WHEN** 上游返回 `RequestTimeTooSkewed` 或 `SignatureDoesNotMatch`
- **THEN** 失败原因 MUST 可读，MUST NOT 被吞成"凭据不可用"

### Requirement: 事件流响应的流式判定

上游响应为二进制事件流（`content-type` 以 `application/vnd.amazon.eventstream` 开头）时，网关 MUST 按流式处理：逐块透传、不缓冲整包，并把该请求标记为流式。请求体声明 `stream = true` 时同样 MUST 标记为流式。

#### Scenario: Bedrock 事件流
- **WHEN** 上游返回 `application/vnd.amazon.eventstream`
- **THEN** 响应按流式透传，`Access` 事件标记为流式请求

#### Scenario: 请求声明流式
- **WHEN** 请求体 `stream = true` 且上游返回事件流
- **THEN** 该请求标记为流式；无 token 用量时按"用量缺失"记账
