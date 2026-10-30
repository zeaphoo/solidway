# gateway-core Specification (delta: fix-usage-token-capture)

## MODIFIED Requirements

### Requirement: 保真转发与白名单改写

网关 MUST 原样转发请求体、响应体与流式事件，响应状态码原样回传；改写白名单为**六项**：

1. 鉴权头：移除入站的 `Authorization` / `x-api-key` / `x-goog-api-key` / `api-key`，替换为所选 Provider 凭据按 `auth_scheme` 构造的头；
2. 目标地址：按「入站家族与上游地址构造」拼接；
3. query 中的凭据类参数：仅移除 `key` 与 `api_key`（大小写不敏感），其余 query 顺序与字面量 MUST 原样保留；
4. 请求中承载客户端模型名的位置（仅模型别名生效时）：请求体顶层 `model`；
5. 响应中回写的模型名（仅模型别名生效时，**仅限成功响应**）：见「保真转发与白名单改写」下各家族说明与「Anthropic Messages 通路」。错误响应体（含 LastFailure 透传的上游错误体）MUST 原样透传，MUST NOT 回写 `model`；
6. 流式用量参数：OpenAI Chat Completions 入站且请求体顶层 `stream = true`、且未声明 `stream_options` 时，向顶层对象末尾插入 `"stream_options":{"include_usage":true}`。

第 6 项 MUST 只做定点插入：不重排既有成员、不重序列化请求体，其他字节 MUST NOT 改动；请求已声明 `stream_options`（无论取值）时 MUST 原样转发。其余字节 MUST NOT 改动；流式响应 MUST NOT 缓冲。除入站鉴权头外的请求头（含 `anthropic-version` / `anthropic-beta` / `accept-encoding`）MUST 原样转发。

顶层 `model` 的改写仅发生在模型别名生效时（`upstream` 与客户端 `name` 不同）：请求把顶层 `model` 的字符串值替换为该 Provider 声明的上游模型 id；**成功响应**把顶层 `model` 回写为客户端模型名；非流式响应 MUST 整体读取后定点替换，流式响应 MUST 按事件行处理且 MUST NOT 缓冲整条流。除上述位置外，MUST NOT 改写嵌套对象或数组中的任何同名字段。

#### Scenario: 未知字段原样到达
- **WHEN** 请求体含网关不认识的字段
- **THEN** 上游收到除白名单改写点外逐字节一致的请求体

#### Scenario: 流式 chat 补 usage 参数
- **WHEN** 客户端 `POST /v1/chat/completions` 且请求体为 `{"model":"m","stream":true}`
- **THEN** 上游收到 `{"model":"m","stream":true,"stream_options":{"include_usage":true}}`，其余字节一致

#### Scenario: 已声明 stream_options
- **WHEN** 客户端请求体已含 `stream_options`（如 `{"include_usage":false}`）
- **THEN** 请求体原样转发，网关 MUST NOT 覆盖客户端取值

#### Scenario: 非流式与非 chat 家族不注入
- **WHEN** 请求体无顶层 `stream = true`，或入站为 Responses / Anthropic Messages
- **THEN** 请求体原样转发，MUST NOT 注入 `stream_options`

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

### Requirement: 用量元数据提取

网关 SHALL 在**不改变线上响应字节**的前提下提取用量元数据：

- 非流式响应：从 JSON 响应中提取 `usage`；
- 流式响应：从 SSE 事件中提取 usage，支持 OpenAI Chat 的 usage chunk、OpenAI Responses 的 `response.completed`、Anthropic 的 `message_start` / `message_delta`；
- 统计 `bytes_out`；
- 无法找到 usage 时 MUST 在完成事件中标记为缺失，MUST NOT 估算 token。

上游响应带 `content-encoding` 时，网关 MUST 先按该头旁路解码再解析 usage，支持 `gzip` / `deflate` / `br` / `zstd`。旁路解码 MUST 只作用于观察副本：转发给客户端的响应字节与 `content-encoding` 头 MUST 原样保留，`bytes_out` MUST 仍按原始（压缩后）字节计数。编码未知或解码失败时 MUST 静默降级为「usage 缺失」，MUST NOT 影响转发结果。非流式旁路解码 MUST 设置明文上限（32 MiB），超限按解码失败处理。

提取过程 MUST NOT 修改响应字节；流式观察 MUST NOT 改变事件内容，也 SHALL NOT 因统计而无限缓冲响应（行缓冲上限 1 MiB）。请求侧唯一的用量相关改动是「保真转发与白名单改写」第 6 项（补 `stream_options.include_usage`）。

#### Scenario: 非流式 usage
- **WHEN** 上游 JSON 响应包含 `usage`
- **THEN** 完成事件携带归一化后的 input / output token

#### Scenario: 流式 usage
- **WHEN** 流式响应最后包含 usage（或 `response.completed`）
- **THEN** 完成事件携带对应 token，且转发字节不变

#### Scenario: 压缩响应取 usage
- **WHEN** 上游以 `content-encoding: gzip` 返回 JSON 或 SSE（响应体含 usage）
- **THEN** 完成事件携带解码后解析出的 token，且客户端收到的仍是原始压缩字节与该头

#### Scenario: 压缩数据损坏
- **WHEN** 响应体与 `content-encoding` 不符（解码失败）
- **THEN** 完成事件标记 usage 缺失、token 为 0；响应仍原样透传

#### Scenario: usage 缺失
- **WHEN** 上游响应没有 usage
- **THEN** 完成事件标记 usage 缺失，token 为 0，不进行估算

#### Scenario: 不修改请求
- **WHEN** 客户端未请求流式 usage 且请求体不含 `stream_options`
- **THEN** 网关只补 `stream_options.include_usage`（见「保真转发与白名单改写」第 6 项），MUST NOT 改写其他请求参数
