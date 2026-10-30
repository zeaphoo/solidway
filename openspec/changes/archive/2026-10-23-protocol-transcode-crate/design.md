# Design — protocol-transcode-crate

状态：draft（试验性 spike；未接入数据面）
日期：2026-10-23
crate：`crates/solidway_transcode`（package `solidway-transcode`）
对应提案：`openspec/changes/2026-10-23-protocol-transcode-crate/proposal.md`
决策：**方案 B —— 每个方向独立实现，方向之间零依赖。**
实现状态（2026-10-23）：M0–M4 已完成；当前不接入 core，接入另立 change + ADR。

## 1. 目标与非目标

### 1.1 目标

- **G1 方向独立**：每个「客户端格式 → 上游协议」组合是一个独立模块，请求、非流式响应、流式事件全部由该模块自己实现；方向之间不得互相引用。
- **G2** 覆盖三种公开格式的 6 个方向：`openai.chat` / `openai.responses` / `anthropic.messages` 两两组合。
- **G3** 每个方向都覆盖请求、响应、流式；流式不缓冲整条流。
- **G4** 私有协议优先用「方言 = base wire + patch」复用 base 方向；结构完全不同的私有协议写自己的方向模块。
- **G5** 有损转换显式化：**方向级**能力矩阵 + `Report` + `Strict` / `BestEffort`；禁止静默丢弃。
- **G6** 纯库：零 IO、无 async runtime、无 HTTP、无 DB、无全局可变状态。
- **G7** 未来可作为 egress 适配器（ADR 0013 所指的独立维度）的唯一协议转换依赖。

### 1.2 非目标

- **不做协议无关 IR**（被否，见 §2.3）。
- 不接入 `solidway_core` / `solidway_api`；不改 `ApiFormat`、转发白名单、数据库、既有 specs。
- 不负责鉴权、URL 拼接、`base_url` / 路径模板、模型别名、选号、换号、重试、超时、用量落库。
- 不做 token 估算；`Usage` 缺失保留为 `None`，不填 0。
- 不做 `/v1/models`、文件上传、Batch、Realtime、Embeddings、Images、Audio 生成等非对话端点。
- 不复制 bifrost / sub2api 源码。

## 2. 决策：方向独立（方案 B）

### 2.1 参考实现

| 项目 | 做法 | 采用情况 |
|---|---|---|
| bifrost（Apache-2.0） | canonical hub：内部统一 `BifrostChatRequest` / `BifrostResponsesRequest`，provider 适配器接 canonical | 不采用其 hub，只参考流式 state machine 与事件组织 |
| sub2api（LGPL-3.0） | `pkg/apicompat` 成对直桥：请求 / 响应 / 流式各自成对实现，fixture 丰富 | 采用「成对」思路；增加方向级 caps、Report、隔离守卫 |

### 2.2 为什么选 B

1. **隔离性**：一个方向的 bug、重构、删除不会影响其他方向。方向模块之间零 `use`。
2. **保真上限高**：每条方向可以针对自己的语义鸿沟做最优映射，不必先投影到一个公共模型。
3. **并行开发**：6 个方向可以由不同人同时推进，fixture 自包含，合并冲突低。
4. **局部优化**：热点方向可以单独优化分配与解析次数，不需要为全局抽象买单。
5. **失败面小**：某个方向不支持某特性时，只影响该方向的 caps 与测试，不会污染其他方向。

### 2.3 被否方案：canonical IR

否决理由：

- IR 天生有损，无法同时表达 `thinking.signature`、`cache_control`、`store/include`、server tools 等协议特有语义；
- 为了避免丢失，IR 会演化成「所有协议字段的并集」，最终变成带 enum 外壳的 JSON；
- 多一次 `wire → IR → wire` 的分配与序列化，流式还要重新合成 id / index / seq；
- 协议演进会牵动 IR 与所有方向的映射代码，方向之间重新耦合，违背本项目的隔离目标。

**代价**：会出现重复代码。接受该代价，用「机械公共层 + 方向级 fixture + 隔离守卫」兜底（见 §5、§15、§16）。

### 2.4 成本模型

| 场景 | 方向模块数量 |
|---|---|
| 3 个公开格式两两组合 | 6 |
| 私有协议是某个公开 base 的方言 | 0 个新方向；1 个方言 patch + registry 组合 |
| 结构不同的私有协议只作为上游 | 3（`chat_to_P` / `anthropic_to_P` / `responses_to_P`） |
| 结构不同的私有协议同时作为客户端 | 再加 3 个反向方向 |
| N 个结构不同的私有上游 | 3N |

这是 B 的显式成本。用方言把大多数私有协议挡在「0 个新方向」这一档；只有真正结构不同的协议才付 3 个方向的成本。

## 3. 术语

| 术语 | 定义 | _Avoid_ |
|---|---|---|
| 方向（direction） | 一个「客户端格式 → 上游协议」的适配单元，包含请求、响应、流式三部分 | 桥、adapter、路由 |
| 方向模块（direction module） | 实现某个方向的独立目录，不得引用其他方向 | 转换器、mapper |
| 客户端格式（src） | 入站格式：`openai.chat` / `openai.responses` / `anthropic.messages` | 入站协议 |
| 上游协议（dst） | Provider 实际使用的协议 | 出站格式 |
| 线上类型（wire） | 与某协议 JSON / SSE 字段一一对应的 serde 类型 | DTO、IR |
| 方言（dialect） | 私有协议以某个 base wire 为骨架，用 patch 描述差异 | 变体、子协议 |
| 机械公共层（common） | 只含分帧、JSON Pointer、id 前缀、Option 算术等无协议语义的工具 | 公共转换层、core |
| 方向能力（direction caps） | 某个方向对每类语义的支持级别 | 协议能力 |
| 必保语义（must-preserve） | 该方向必须逐字或等价保留的语义清单，测试门禁 | 关键字段 |
| 转换报告（report） | 本次转换的 warning / dropped 路径 | 日志、诊断 |
| 组合方向（composed direction） | 由「base 方向 + 方言 patch」在 registry 中拼出的方向 | 继承、宏 |

## 4. 方向语义（最关键）

约定：

- `src` = **客户端格式**（入站）。
- `dst` = **上游协议**（Provider）。
- `request` 转换：`src → dst`。
- `response` / `stream` 转换：`dst → src`（响应是反方向）。

所以一个方向模块同时包含两个数据流方向，但只有一个「路由方向」。例如 `chat_to_anthropic`：

~~~rust
let dir = direction(OPENAI_CHAT, ANTHROPIC_MESSAGES).unwrap();

// 请求：Chat -> Anthropic
let upstream_req = dir.request(client_body, &plan)?;

// 响应：Anthropic -> Chat
let client_resp = dir.response(upstream_body, &plan)?;

// 流式：Anthropic 帧 -> Chat 帧
let mut stream = dir.stream(&plan)?;
~~~

6 个公开方向：

| 方向模块 | 客户端格式（src） | 上游协议（dst） | request 方向 | response / stream 方向 |
|---|---|---|---|---|
| `chat_to_anthropic` | Chat | Anthropic | Chat → Anthropic | Anthropic → Chat |
| `anthropic_to_chat` | Anthropic | Chat | Anthropic → Chat | Chat → Anthropic |
| `chat_to_responses` | Chat | Responses | Chat → Responses | Responses → Chat |
| `responses_to_chat` | Responses | Chat | Responses → Chat | Chat → Responses |
| `anthropic_to_responses` | Anthropic | Responses | Anthropic → Responses | Responses → Anthropic |
| `responses_to_anthropic` | Responses | Anthropic | Responses → Anthropic | Anthropic → Responses |

这张表是后续所有实现与测试的基准；方向命名永远读作「客户端 → 上游」。

## 5. 架构与依赖规则

### 5.1 分层

~~~txt
lib.rs
  └── registry ──▶ directions/<src>_to_<dst>/ ──▶ wire/<protocol>
        │                    │
        │                    └──▶ common/（仅机械工具）
        └── dialect/ ──▶ wire/<base>
~~~

### 5.2 依赖矩阵

| 模块 | 允许依赖 | 禁止依赖 |
|---|---|---|
| `common/` | 标准库、`serde_json`、`bytes`、`thiserror` | `wire/`、`directions/`、`dialect/` |
| `wire/<protocol>` | `serde`、`common/` 中纯类型 | `directions/`、`dialect/` |
| `directions/<dir>/` | `wire/<src>`、`wire/<dst>`、`common/`、`report`、`protocol` | **其他 `directions/`** |
| `dialect/<name>/` | 其 base 的 `wire/`、`common/`、`protocol` | `directions/` |
| `registry/` | `directions/`、`dialect/`、`protocol` | 转换逻辑（只做查找与组合） |

硬规则：

1. 方向模块之间零引用。删掉任意一个方向目录，其他方向仍可编译与测试。
2. `common/` 不得出现协议名、字段名或 `if protocol == ...`。
3. 方向模块不得直接调用其他方向的 `request` / `response` / 流式函数。
4. 方言模块不得知道任何方向；组合只发生在 `registry`。
5. `wire/` 只做序列化/反序列化，不写业务默认值与跨协议判断。

### 5.3 为什么这样隔离

- 方向是产品行为的最小可回滚单元：一个方向出问题，可以单独下线或回退。
- 私有协议的长尾由一个方向一个目录吸收，不会把公共抽象拖成并集。
- 测试、fixture、caps、必保清单都归属方向，评审范围清晰。

## 6. 方向模块契约

~~~rust
pub struct Plan {
    /// 客户端格式。
    pub src: ProtocolId,
    /// 上游协议。
    pub dst: ProtocolId,
    pub policy: TranscodePolicy,
}

pub struct Transcoded<T> {
    pub body: T,
    pub report: Report,
}

pub trait Direction: Send + Sync {
    fn src(&self) -> ProtocolId;
    fn dst(&self) -> ProtocolId;
    fn caps(&self) -> &'static DirectionCaps;

    /// src 请求 -> dst 请求。
    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>>;

    /// dst 响应 -> src 响应。
    fn response(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>>;

    /// dst 流式帧 -> src 流式帧。
    fn stream(&self, plan: &Plan) -> Result<Box<dyn StreamBridge>>;
}

pub trait StreamBridge {
    fn push(&mut self, frame: InFrame) -> Result<SmallVec<[OutFrame; 4]>>;
    fn finish(&mut self) -> Result<SmallVec<[OutFrame; 4]>>;
    /// 流式告警在流结束后读取。
    fn report(&self) -> &Report;
}
~~~

- 每个方向是一个零大小结构体，`impl Direction` 后注册到静态表。
- `direction(src, dst)` 线性查表；6 个公开方向 + 方言组合，规模很小。
- `src == dst` 未定义；调用方应直接透传，crate 不处理同协议。
- `Direction` 的公开与否见 §20 未决问题；spike 阶段只公开 `direction()` 与字节 API。

## 7. wire 类型

每个协议一个独立模块，忠实映射线上字段：

~~~rust
// wire/openai_chat.rs
pub(crate) struct ChatRequest { /* ... */ }
pub(crate) struct ChatResponse { /* ... */ }
pub(crate) struct ChatChunk { /* ... */ }

// wire/openai_responses.rs
pub(crate) struct ResponsesRequest { /* ... */ }
pub(crate) struct ResponsesResponse { /* ... */ }
pub(crate) enum ResponsesEvent { /* ... */ }

// wire/anthropic.rs
pub(crate) struct AnthropicRequest { /* ... */ }
pub(crate) struct AnthropicResponse { /* ... */ }
pub(crate) enum AnthropicEvent { /* ... */ }
~~~

规则：

- 不复用跨协议的「消息」「内容块」类型；每个 wire 只描述自己的协议。重复的 `Text` / `Image` 结构是隔离成本，接受。
- `wire` 类型 `pub(crate)`；对外只暴露字节 API。
- 每个 wire 自带错误外壳类型，不做跨协议错误抽象。
- 未知字段：serde 保留为 `serde_json::Value` 或 `#[serde(flatten)] extras`，由方向决定丢弃并写 warning。

## 8. 公开方向明细

> 每个方向只列「关键映射、必保语义、已知有损点」。通用语义参考见附录 A；实现以各自 fixture 为准。

### 8.1 `chat_to_anthropic`

- **request（Chat → Anthropic）**：system 消息抽到顶层 `system`；`tool_calls` → `tool_use`；`role=tool` → `user` + `tool_result`；`max_completion_tokens` → `max_tokens`（Anthropic 必填，缺失时按方言默认并写 warning）；`stop` → `stop_sequences`；`top_k` 保留；`n` / `logprobs` / `seed` / penalties 丢弃。
- **response（Anthropic → Chat）**：content block → Chat message；`tool_use` → `tool_calls`；`thinking` → `reasoning_content`（Lossy）；`stop_reason` → `finish_reason`；usage 映射。
- **stream（Anthropic → Chat）**：`message_start` → 首 chunk；`content_block_*` → `delta.content` / `delta.tool_calls`；`message_delta` → `finish_reason`；`message_stop` → `[DONE]`。
- **必保**：文本、system、`tool_use` / `tool_result` 配对、图片、`stop`、thinking 文本。
- **有损**：thinking signature（Chat 无标准字段）、`cache_control`、citations、server tools。
- **fixture**：`tests/golden/chat_to_anthropic/`。

### 8.2 `anthropic_to_chat`

- **request（Anthropic → Chat）**：顶层 `system` → system 消息；`tool_use` → `tool_calls`；`tool_result` → `role=tool`；`max_tokens` → `max_completion_tokens`；`stop_sequences` → `stop`；`top_k` 丢弃；`thinking` → `reasoning_content`（Lossy）。
- **response（Chat → Anthropic）**：choice → content blocks；`tool_calls` → `tool_use`；`finish_reason` → `stop_reason`；usage 映射。
- **stream（Chat → Anthropic）**：首 chunk → `message_start`；`delta.content` → `text_delta`；`delta.tool_calls` → `input_json_delta`；`finish_reason` → `message_delta` + `message_stop`。
- **必保**：system、工具配对、图片、`stop`。
- **有损**：`top_k`、`cache_control`、thinking signature、`n > 1`。
- **fixture**：`tests/golden/anthropic_to_chat/`。

### 8.3 `chat_to_responses`

- **request（Chat → Responses）**：`messages` → `input`；system 消息 → `instructions`；function tool 直转；`tool_choice` 映射；`response_format` → `text.format`；`max_completion_tokens` → `max_output_tokens`；`stop` 不支持；`n` / `logprobs` / `seed` / penalties 不支持。
- **response（Responses → Chat）**：output item → Chat message / `tool_calls`；reasoning summary → `reasoning_content`（Lossy）；`status` → `finish_reason`；usage 映射。
- **stream（Responses → Chat）**：`response.created` → 首 chunk；`output_text.delta` → `delta.content`；`function_call_arguments.delta` → `delta.tool_calls`；`response.completed` → `finish_reason` + `[DONE]`。
- **必保**：文本、工具定义、`tool_choice`、结构化输出。
- **有损**：encrypted reasoning、citations、server tools、`stop`。
- **fixture**：`tests/golden/chat_to_responses/`。

### 8.4 `responses_to_chat`

- **request（Responses → Chat）**：`instructions` → system 消息；`input` item → messages；`function_call` → assistant `tool_calls`；`function_call_output` → `role=tool`；`max_output_tokens` → `max_completion_tokens`；`text.format` → `response_format`；`store` / `include` / `previous_response_id` 不支持。
- **response（Chat → Responses）**：Chat message / `tool_calls` → output items；`reasoning_content` → reasoning summary（Lossy）；`finish_reason` → `status`。
- **stream（Chat → Responses）**：首 chunk → `response.created`；`delta.content` → `output_text.delta`；`delta.tool_calls` → `function_call_arguments.delta`；`finish_reason` → `response.completed`。
- **必保**：instructions、input item 顺序、工具、`tool_choice`。
- **有损**：`store`、`include`、`previous_response_id`、custom tools、encrypted reasoning。
- **fixture**：`tests/golden/responses_to_chat/`。

### 8.5 `anthropic_to_responses`

- **request（Anthropic → Responses）**：顶层 `system` → `instructions`；messages → `input`；`tool_use` → `function_call`；`tool_result` → `function_call_output`；`max_tokens` → `max_output_tokens`；`thinking` → `reasoning.effort` / summary（Lossy）；`cache_control` 无等价字段；`stop_sequences` 不支持。
- **response（Responses → Anthropic）**：output item → content block；`function_call` → `tool_use`；reasoning item 的 `encrypted_content` → thinking signature envelope（**必保**）；`status` → `stop_reason`。
- **stream（Responses → Anthropic）**：`response.created` → `message_start`；`output_text.delta` → `text_delta`；`function_call_arguments.delta` → `input_json_delta`；`response.completed` → `message_delta` + `message_stop`。
- **必保**：thinking signature（通过 envelope 编码/解码）、工具、system。
- **有损**：`cache_control`、server tools、`stop`。
- **fixture**：`tests/golden/anthropic_to_responses/`。

### 8.6 `responses_to_anthropic`

- **request（Responses → Anthropic）**：`instructions` → 顶层 `system`；`input` item → messages；`function_call` → `tool_use`；`function_call_output` → `tool_result`；`max_output_tokens` → `max_tokens`；reasoning → `thinking.budget_tokens`（Lossy）；custom tools 不支持；`store` 忽略。
- **response（Anthropic → Responses）**：content block → output item；`thinking` → reasoning summary + `encrypted_content` envelope（**必保**）；`tool_use` → `function_call`；`stop_reason` → `status`。
- **stream（Anthropic → Responses）**：`message_start` → `response.created`；`text_delta` → `output_text.delta`；`input_json_delta` → `function_call_arguments.delta`；`message_stop` → `response.completed`。
- **必保**：instructions、工具、thinking signature。
- **有损**：custom tools、`store` / `include`、`cache_control`。
- **fixture**：`tests/golden/responses_to_anthropic/`。

## 9. 流式与帧

### 9.1 分帧层（common）

分帧是字节层问题，与协议语义无关，放在 `common/frame.rs`：

~~~rust
pub struct FrameDecoder { /* SSE / AWS eventstream */ }
impl FrameDecoder {
    pub fn feed(&mut self, chunk: &[u8]) -> Result<SmallVec<[InFrame; 4]>>;
    pub fn finish(&mut self) -> Result<SmallVec<[InFrame; 4]>>;
}

pub struct FrameEncoder { /* SSE / AWS eventstream */ }
impl FrameEncoder {
    pub fn encode(&mut self, frame: &OutFrame) -> Result<Bytes>;
}

pub enum InFrame {
    Sse { event: Option<String>, id: Option<String>, data: Bytes },
    AwsEventStream { headers: Vec<(String, String)>, payload: Bytes },
    Eof,
}

pub enum OutFrame {
    Sse { event: Option<String>, id: Option<String>, data: Bytes },
    AwsEventStream { headers: Vec<(String, String)>, payload: Bytes },
    Done, // 仅 Chat 目标
}
~~~

- SSE：`event:` / `data:` / `id:` / 注释 / 多行 data / `[DONE]` / 空行分帧。
- AWS eventstream：prelude、total length、headers、payload、CRC。
- 解码器只分帧，不解释 JSON；协议事件解析在方向模块内。
- 编码器只分帧，不改变 `OutFrame`。

### 9.2 方向内状态机

每个方向的 `stream.rs` 自己实现状态机，**不共享跨方向事件逻辑**。不变量：

1. 单次 `push` 只依赖有限状态，禁止累积完整消息文本。
2. 终止事件恰好一次。
3. 目标为 Chat 时，`Done` 只在收到上游终止事件或 `finish()` 后发出。
4. 目标为 Anthropic / Responses 时，补齐生命周期事件，即使上游首帧已带内容。
5. 未知事件：Strict 报错；BestEffort 丢弃 + warning。
6. 上游 `error` 事件映射为目标协议错误事件并立即终止。
7. keepalive / ping：目标支持则透传，否则丢弃并只记一次 warning。
8. 上游在终止事件前 EOF：`finish()` 必须产出失败 / incomplete 终止，不得静默结束。
9. 同一输入两次运行输出逐字节一致（确定性）。

事件映射参考见附录 B；每个方向以自身 fixture 为准。

## 10. 方向级能力矩阵

能力矩阵按**方向**声明，而不是按协议声明。原因：同一个协议在不同方向上的可表达能力不同；按协议声明会产生「协议支持但该方向没实现」的假阳性。

~~~rust
pub enum Support {
    Native,
    Emulated,
    Lossy(&'static str),
    Unsupported(&'static str),
}

pub struct DirectionCaps {
    pub request: RequestCaps,
    pub response: ResponseCaps,
    pub stream: StreamCaps,
}

pub struct RequestCaps {
    pub system_prompt: Support,
    pub developer_role: Support,
    pub images: Support,
    pub documents: Support,
    pub audio_input: Support,
    pub tools: Support,
    pub parallel_tool_calls: Support,
    pub custom_tools: Support,
    pub hosted_tools: Support,
    pub tool_choice: Support,
    pub thinking: Support,
    pub cache_control: Support,
    pub structured_output: Support,
    pub stop: Support,
    pub top_k: Support,
    pub seed: Support,
    pub penalties: Support,
    pub n_choices: Support,
    pub logprobs: Support,
}

pub struct ResponseCaps {
    pub thinking: Support,
    pub thinking_signature: Support,
    pub citations: Support,
    pub refusal: Support,
    pub server_tools: Support,
    pub usage_cache_tokens: Support,
    pub usage_reasoning_tokens: Support,
}

pub struct StreamCaps {
    pub text: Support,
    pub thinking: Support,
    pub tool_calls: Support,
    pub parallel_tool_calls: Support,
    pub usage: Support,
    pub errors: Support,
    pub keepalive: Support,
}
~~~

规则：

- 每个方向一份静态 `DirectionCaps`，由 `Direction::caps()` 返回。
- `Support` 按 `Native < Emulated < Lossy < Unsupported` 实现顺序；组合方向取每项的较宽者（更差者）。
- 矩阵测试遍历每个字段：`Unsupported` 在 Strict 下必须报错，在 BestEffort 下必须产生 warning；其余级别必须能产出目标字节。
- 新增字段必须同时补所有方向的能力声明与测试，否则矩阵测试失败。
- 方言组合方向的能力 = base 方向与 dialect 的**逐项收窄**（`Unsupported` > `Lossy` > `Emulated` > `Native`）。

## 11. 报告与错误

~~~rust
pub struct Report { pub warnings: Vec<Warning> }

pub struct Warning {
    pub code: WarningCode,
    pub side: Side,          // Source | Target
    pub pointer: String,     // JSON Pointer
    pub detail: String,
}

pub enum WarningCode {
    Dropped,
    Degraded,
    Synthesized,
    UnknownField,
    UnknownEvent,
    UsageMissing,
    IdSynthesized,
    Truncated,
}
~~~

- 每个方向自己决定 warning 的位置与文案；`Pointer` 相对 `side` 指向的文档。
- 两种策略：
  - `Strict`：`Unsupported` 直接返回错误，不产出目标字节；`Lossy` 允许但必须 warning。
  - `BestEffort`：按 caps 降级，所有降级 / 丢弃写 warning。
- 两种模式都禁止静默丢弃；`Report` 是 `Transcoded<T>` 的一部分。
- 错误映射由方向自己实现（协议错误外壳差异大，不做共享语义映射）；`common` 只提供机械的 JSON 拼装。

## 12. 公开 API

~~~rust
pub fn direction(src: ProtocolId, dst: ProtocolId) -> Option<&'static dyn Direction>;

pub struct Plan {
    pub src: ProtocolId,
    pub dst: ProtocolId,
    pub policy: TranscodePolicy,
}

pub struct Transcoded<T> {
    pub body: T,
    pub report: Report,
}
~~~

- 只公开字节 API；`wire` / `directions` / `dialect` 模块 `pub(crate)`。
- `src == dst` 未定义，调用方直接透传。
- `direction()` 找不到时返回 `None`，由调用方决定报错或回退；crate 不抛「协议不支持」的全局错误。

## 13. crate 布局

~~~txt
crates/solidway_transcode/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── protocol.rs                  # ProtocolId / WireKind / Support / DirectionCaps
│   ├── report.rs                    # Report / Warning / TranscodePolicy
│   ├── registry.rs                  # 静态方向表 + StreamTranslator（方言组合待实现）
│   ├── common/
│   │   ├── mod.rs
│   │   ├── empty.rs                 # 空响应合成
│   │   ├── frame.rs                 # FrameDecoder / FrameEncoder（SSE）
│   │   ├── json.rs                  # JSON Pointer 解析（RFC 6901）
│   │   ├── model.rs                 # 模型名分类 / 通用阈值
│   │   ├── schema.rs                # JSON Schema 归一化（root union / $ref）
│   │   └── usage.rs                 # Option<u64> 饱和算术
│   ├── wire/
│   │   ├── mod.rs
│   │   ├── openai_chat/{mod.rs,types.rs,request.rs,response.rs,stream.rs,error.rs}
│   │   ├── openai_responses/{mod.rs,types.rs,request.rs,response.rs,event.rs,error.rs}
│   │   └── anthropic/{mod.rs,types.rs,request.rs,response.rs,event.rs,error.rs}
│   ├── directions/
│   │   ├── mod.rs
│   │   ├── chat_to_anthropic/{mod.rs,request/{mod.rs,content.rs,params.rs},response.rs,stream.rs}
│   │   ├── anthropic_to_chat/{mod.rs,request/{mod.rs,content.rs},response.rs,stream.rs}
│   │   ├── chat_to_responses/{mod.rs,request.rs,response.rs,stream.rs}
│   │   ├── responses_to_chat/{mod.rs,request.rs,response.rs,stream/{mod.rs,emit.rs}}
│   │   ├── anthropic_to_responses/{mod.rs,request.rs,response.rs,stream.rs}
│   │   └── responses_to_anthropic/{mod.rs,request/{mod.rs,content.rs},response.rs,stream/{mod.rs,emit.rs}}
│   └── dialect/
│       ├── mod.rs                   # Dialect trait / all()
│       ├── patch.rs                 # FieldOp 与 JSON Pointer patch
│       ├── codex.rs                 # base = openai.responses
│       └── claude_code.rs           # base = anthropic.messages
└── tests/
    ├── fixtures/*.json / *.txt      # 三个协议的请求 / 响应 / 流式样例
    ├── directions.rs                # 6 方向 × 请求 / 响应 / 流式
    └── isolation.rs                 # 依赖隔离守卫
~~~

> 实现时按「请求 / 内容 / 参数 / emit」拆分，保证单文件 ≤ 500 行。

约束：

- 单文件 ≤ 500 行；一个方向的请求 / 响应 / 流式 / fidelity 分开文件。
- `directions/<dir>/` 是自包含单元：代码、tests fixture、caps、必保清单都在该方向名下。
- 拟用依赖：`serde` / `serde_json` / `thiserror` / `bytes` / `indexmap` / `smallvec` / `tracing`；不含 `tokio` / `axum` / `reqwest` / `futures` / `sqlx`。新增依赖按 AGENTS.md 确认。

## 14. 私有协议与方言

### 14.1 方言定义

方言只描述「与 base wire 的形状差异」，不描述语义转换：

~~~rust
pub trait Dialect: Send + Sync {
    fn base(&self) -> ProtocolId;
    fn id(&self) -> ProtocolId;

    fn decode_request(&self, p: &[u8]) -> Result<Vec<u8>>;   // P -> B
    fn encode_request(&self, b: &[u8]) -> Result<Vec<u8>>;   // B -> P
    fn decode_response(&self, p: &[u8]) -> Result<Vec<u8>>;  // P -> B
    fn encode_response(&self, b: &[u8]) -> Result<Vec<u8>>;  // B -> P
    fn decode_frame(&self, p: InFrame) -> Result<SmallVec<[InFrame; 4]>>;  // P -> B
    fn encode_frame(&self, b: OutFrame) -> Result<SmallVec<[OutFrame; 4]>>; // B -> P
}
~~~

`FieldOp`：`Rename` / `Wrap` / `Unwrap` / `Default` / `Set` / `Drop`；只允许形状操作。

### 14.2 registry 组合

设 `P` 是 base `B` 的方言，`S` 是任意公开格式：

| 目标方向 | 组合方式 |
|---|---|
| `S → P`（S != B） | request：`S→B` 方向 + `B→P` patch；response/stream：`P→B` patch + `S→B` 方向反向 |
| `S → P`（S == B） | 只有 patch：request `B→P`，response/stream `P→B` |
| `P → S`（S != B） | request：`P→B` patch + `B→S` 方向；response/stream：`B→S` 方向反向 + `B→P` patch |
| `P → S`（S == B） | 只有 patch：request `P→B`，response/stream `B→P` |
| `P1 → P2`（同 base） | `P1→B` patch + `B→P2` patch |
| `P1 → P2`（不同 base） | request：`P1→B1` decode + `B1→B2` 方向 request + `B2→P2` encode；response / stream：`P2→B2` decode + `B1→B2` 方向 response / stream + `B1→P1` encode |

组合只发生在 `registry`；方向模块与方言模块互不引用。组合方向的能力按 §10 逐项收窄。

### 14.3 首批 pilot（待确认）

| pilot | base | 验证点 |
|---|---|---|
| `private.codex` | `openai.responses` | `store: false`、`reasoning.encrypted_content`、custom/grammar tools、`previous_response_id` |
| `private.claude-code` | `anthropic.messages` | thinking + signature、`cache_control`、server tools、beta 结构差异 |

### 14.4 结构不同的私有协议

不走方言，直接写方向模块，例如 `chat_to_bedrock` / `bedrock_to_chat` / `anthropic_to_bedrock` / `bedrock_to_anthropic`。如果某个协议只作为上游，通常只需要 3 个方向（三个公开客户端各一个）。成本见 §2.4。

## 15. 测试设计

### 15.1 方向 fixture（自包含）

~~~txt
tests/golden/<direction>/<case>/
├── request.src.json
├── request.dst.json
├── response.src.json
├── response.dst.json
├── stream.src.sse
├── stream.dst.sse
└── expect.json
~~~

- `request.src.json` 是**客户端**请求；`request.dst.json` 是**上游**期望请求。
- `response.src.json` 是**客户端**期望响应；`response.dst.json` 是**上游**响应。
- `stream.src.sse` 是**客户端**期望流；`stream.dst.sse` 是**上游**流。
- JSON 语义比较；SSE 逐帧比较 `event` + `data`。
- `expect.json`：`warnings` / `error` / `usage` / `note`。
- fixture 覆盖：纯文本、system / developer、图片、文档、单工具、并行工具、工具结果、thinking、cache_control、拒绝、引用、三种 usage、上游错误、流截断。

### 15.2 必保语义测试

每个方向的 `fidelity.rs` 声明：

~~~rust
pub const MUST_PRESERVE: &[&str] = &["text", "system", "tool_pairing", "images"];
pub const KNOWN_LOSSY: &[&str] = &["thinking_signature", "cache_control"];
~~~

`conformance.rs` 断言：

- `MUST_PRESERVE` 的每一项至少有一个 fixture 覆盖，且断言通过；
- `KNOWN_LOSSY` 的每一项至少有一个 fixture 产生对应 warning；
- 不允许出现未声明的 warning。

### 15.3 流式测试

每个方向独立验证：

- 事件顺序与 fixture 一致；
- id / index / seq 稳定（同输入两次运行逐字节一致）；
- 终止事件恰好一次；
- 截断输入得到失败 / incomplete 终止；
- Strict 下未知事件报错；BestEffort 下产生 warning；
- keepalive / ping 策略符合 caps。

### 15.4 隔离测试

`tests/isolation.rs` 扫描 `src/`：

- `src/directions/<a>/` 不得出现 `crate::directions::<b>`（`a != b`）；
- `src/common/` 不得出现 `crate::wire` / `crate::directions` / `crate::dialect`；
- `src/wire/` 不得出现 `crate::directions` / `crate::dialect`；
- `src/dialect/` 不得出现 `crate::directions`。

验收：删除任一方向目录，其余方向的单元测试与编译仍通过（CI 用临时脚本验证，不提交删除）。

### 15.5 健壮性与确定性

- `proptest`：随机合法请求 → 目标 JSON 可被目标 wire 反序列化；随机字节不 panic。
- 类型错配：JSON 合法但字段类型错误，返回 `Error::Invalid`，不 panic、不静默忽略。
- 确定性：同一输入重复转换，输出字节一致（流式逐帧一致）。
- 全程本地，不联网；参考实现只用于行为对照。

## 16. 隔离守卫

1. 模块可见性：`wire` / `directions` / `dialect` 全部 `pub(crate)`；只有 `direction()`、`Plan`、`Transcoded`、`Report`、`ProtocolId`、`Support` 公开。
2. `tests/isolation.rs` 做源码扫描，命中禁止依赖即失败。
3. CI 增加 `scripts/check-transcode-isolation.sh`（或复用 `tests/isolation.rs`），检查跨方向引用、`common` 语义泄漏。
4. 评审规则：需要跨方向共享的代码，只有「完全机械、无协议语义」才允许进 `common`；否则复制到各自方向，并在两处注释里互相链接，明确这是隔离成本。

## 17. 与未来 core 的集成边界（本次不实施）

1. `ProviderEntry` 增加 `upstream_protocol`（独立维度），不复用 `api_formats`（沿用 ADR 0013）。
2. 转发时：`inbound.format == upstream.protocol` → 现状字节直通；不等价时 `direction(inbound, upstream)` 取方向模块。
3. `Plan` 由 core 根据入站格式与 Provider 协议构造；crate 不感知路由 / 选号 / 重试。
4. 方向的 `Report` 进入 `CoreEvent`（或新的诊断事件）；用量从目标响应提取。
5. 换号语义不变：仍只在首个字节之前切换；转换发生在最终选定 Provider 之后。

## 18. 里程碑

| 阶段 | 内容 | 退出条件 |
|---|---|---|
| M0 | crate 骨架、`protocol` / `report` / `common/frame`、3 个 wire、`chat_to_anthropic` 的请求 / 响应 / fixture | 单方向全绿，证明目录与契约可复制 |
| M1 | 其余 5 个方向的非流式请求 / 响应 + caps + conformance | 6 方向非流式全绿；方向之间零引用 |
| M2 | 6 个方向的流式状态机 + 顺序 / 终止 / 截断测试 | 6 方向流式全绿；确定性通过 |
| M3 | 方言框架 + `codex` / `claude-code` pilot + registry 组合 | 组合方向 fixture 全绿；base 方向零改动 |
| M4 | 健壮性 / 性能预算 / 隔离守卫 / 设计复查 | proptest 绿；删除方向目录验证通过；输出是否接入 core 的结论 |

M1 的 5 个方向、M2 的 6 个方向可以并行开发；每个方向一个目录、一份 fixture，互不阻塞。

## 19. 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| 重复代码 / 语义漂移 | 同一语义在不同方向表现不一致 | 方向级 fixture、必保清单、caps 门禁；机械工具进 `common`，语义逻辑允许重复并互链注释 |
| 结构不同的私有协议导致 3N 成本 | 长尾维护量大 | 优先方言；只有真正结构不同的才写方向；成本在立项时显式评估 |
| `common` 逐渐语义化 | 隔离被侵蚀 | `tests/isolation.rs` + 评审规则：`common` 不得出现协议名 |
| 方向间无法复用错误 / usage 映射 | 重复实现 | 接受重复；每个方向用 fixture 固化；不引入跨方向语义抽象 |
| 测试量随方向增长 | CI 变慢 | 方向可独立 `cargo test -p solidway-transcode directions::<dir>`；fixture 按目录拆分 |
| 公开 API 过早冻结 | semver 负担 | spike 阶段只公开字节 API；`Direction` trait 暂不承诺稳定 |
| 流式状态机各写一遍 | bug 重复 | 统一不变量 + 每方向顺序 / 截断 / 确定性测试 |

## 20. 未决问题

1. 首批私有协议是否确定为 `codex` / `claude-code`？其他候选（Gemini CLI、Antigravity、Kimi、Grok、Bedrock）的优先级？
2. `count_tokens` 端点跨协议如何处理：不支持，还是引入本地 tokenizer 只做计数？
3. `Direction` trait 是否公开，允许仓库外新增方向？spike 阶段建议不公开。
4. 方言 patch 能否覆盖 `codex` / `claude-code` 的全部形状差异？M3 验证；不够则允许方言内写少量钩子函数。
5. 方向命名用 `directions/` 还是 `bridges/`？建议 `directions/`，与「客户端 → 上游」语义一致。
6. 是否为「同协议但不同版本」（Anthropic beta）建模？建议请求头透传，crate 不感知版本。
7. 是否需要跨方向一致性冒烟测试（如 `A→B→A` 不 panic、不新增字段）？建议加，但不作为共享实现。
8. 错误 / usage 映射是否需要在方向之间做人工对照表？建议用附录 A 作为评审参考，不共享代码。

## 附录 A：语义映射参考

> 仅作为实现与评审的参考，**不是共享抽象**。每个方向以自身 fixture 与 caps 为准，允许合理差异。

### A.1 角色

| 语义 | Chat | Responses | Anthropic |
|---|---|---|---|
| System | `system` 消息 | 顶层 `instructions` | 顶层 `system` |
| Developer | `developer` 消息 | `instructions` 或 developer 项 | 合并进 `system`（Lossy） |
| User | `user` | `input` role=user | `user` |
| Assistant | `assistant` | `input` role=assistant / output message | `assistant` |
| Tool | `tool` | `function_call_output` | `user` 内的 `tool_result` |

### A.2 内容

| 语义 | Chat | Responses | Anthropic |
|---|---|---|---|
| 文本 | string 或 `{type:text}` | `input_text` / `output_text` | `text` |
| 图片 | `image_url` | `input_image` | `image` |
| 文件 | `file` | `input_file` | `document` |
| 音频 | `input_audio` | `input_audio` | 无 |
| Thinking | `reasoning_content`（非标准） | `reasoning` summary + `encrypted_content` | `thinking` / `redacted_thinking` |
| 工具调用 | `tool_calls[]` | `function_call` | `tool_use` |
| 工具结果 | role=tool | `function_call_output` | `tool_result` |
| 拒绝 | `refusal` | `refusal` | 降级为 text |
| 引用 | `annotations` | `output_text.annotations` | `citations` |
| Server tool | 无 | 原生 | 原生 |
| 自定义工具 | 无 | `custom` + grammar | 无 |

### A.3 参数

| 语义 | Chat | Responses | Anthropic |
|---|---|---|---|
| 输出上限 | `max_completion_tokens` / `max_tokens` | `max_output_tokens` | `max_tokens`（必填） |
| temperature / top_p | 同名 | 同名 | 同名 |
| top_k | 无 | 无 | `top_k` |
| stop | `stop` | 无 | `stop_sequences` |
| seed | `seed` | 无 | 无 |
| penalties | 同名 | 无 | 无 |
| reasoning effort | `reasoning_effort`（部分上游） | `reasoning.effort` | `thinking.budget_tokens`（Lossy） |
| reasoning summary | 无 | `reasoning.summary` | 无 |
| 结构化输出 | `response_format` | `text.format` | `output_config.format` |
| parallel_tool_calls | 同名 | 同名 | 无 |
| n / logprobs | 同名 | 无 | 无 |

### A.4 结束原因

| 语义 | Chat | Responses | Anthropic |
|---|---|---|---|
| 自然结束 | `stop` | `completed` | `end_turn` |
| 长度 | `length` | `incomplete` / `max_output_tokens` | `max_tokens` |
| 工具调用 | `tool_calls` | `completed`（含 function_call） | `tool_use` |
| 内容过滤 | `content_filter` | `incomplete` / `content_filter` | `refusal`（Lossy） |
| 拒绝 | `stop` + refusal | `incomplete` / `content_filter` | `refusal` |
| 暂停 | `stop` | `incomplete` | `pause_turn` |
| 错误 | 错误体 | `failed` | `error` |

### A.5 用量

| 语义 | Chat | Responses | Anthropic |
|---|---|---|---|
| 输入 | `prompt_tokens` | `input_tokens` | `input_tokens` |
| 输出 | `completion_tokens` | `output_tokens` | `output_tokens` |
| 缓存读 | `prompt_tokens_details.cached_tokens` | `input_tokens_details.cached_tokens` | `cache_read_input_tokens` |
| 缓存写 | 无 | 无 | `cache_creation_input_tokens` |
| 推理 | `completion_tokens_details.reasoning_tokens` | `output_tokens_details.reasoning_tokens` | 含在 output |
| 总计 | `total_tokens` | `total_tokens` | 目标无字段，按需相加（Emulated） |

`Option` 缺失保留为 `None`；目标协议强制要求数字时省略字段，不填 0。

### A.6 错误外壳

| 语义 | Chat / Responses | Anthropic |
|---|---|---|
| 无效请求 | `invalid_request_error` | `invalid_request_error` |
| 鉴权 | `authentication_error` | `authentication_error` |
| 限流 | `rate_limit_error` | `rate_limit_error` |
| 过载 | `server_error` / `overloaded_error` | `overloaded_error` |
| 超时 | `server_error` | `api_error` |
| 内容过滤 | `content_filter` | `invalid_request_error` |
| 上游错误 | 原样或 `server_error` | `api_error` |

## 附录 B：流式事件参考

> 方向模块各自实现；本附录只描述期望的事件对应关系，具体 id / index / seq 以目标协议官方定义与 fixture 为准。

### B.1 Chat ↔ Anthropic

| Chat | Anthropic |
|---|---|
| 首帧（role=assistant） | `message_start` |
| `delta.content` 首段 | `content_block_start`(text) + `content_block_delta`(text_delta) |
| `delta.content` 后续 | `content_block_delta`(text_delta) |
| `delta.reasoning_content` | `content_block_start`(thinking) + `thinking_delta` |
| `delta.tool_calls[i]` 首帧 | `content_block_start`(tool_use) + `input_json_delta` |
| `delta.tool_calls[i]` 后续 | `input_json_delta` |
| `finish_reason` | `content_block_stop` → `message_delta` → `message_stop` |
| `[DONE]` | 无对应 |

### B.2 Chat ↔ Responses

| Chat | Responses |
|---|---|
| 首帧 | `response.created` |
| role=assistant | `output_item.added`(message) + `content_part.added` |
| `delta.content` | `response.output_text.delta` |
| `delta.reasoning_content` | `response.reasoning_summary_text.delta` |
| `delta.tool_calls` | `response.output_item.added`(function_call) + `response.function_call_arguments.delta` |
| `finish_reason` | `output_text.done` / `content_part.done` / `output_item.done` → `response.completed` |

### B.3 Anthropic ↔ Responses

| Anthropic | Responses |
|---|---|
| `message_start` | `response.created` |
| `content_block_start`(text) | `output_item.added`(message) + `content_part.added` |
| `content_block_delta`(text_delta) | `response.output_text.delta` |
| `content_block_start`(thinking) | `output_item.added`(reasoning) |
| `thinking_delta` | `response.reasoning_summary_text.delta` |
| `content_block_start`(tool_use) | `output_item.added`(function_call) |
| `input_json_delta` | `response.function_call_arguments.delta` |
| `content_block_stop` | 对应的 `*.done` |
| `message_delta` | 记录 finish / usage |
| `message_stop` | `response.completed` / `response.incomplete` |

## 21. M4 结论（2026-10-23）

- **暂不接入 `solidway_core`**：crate 先以独立字节 API 存在，避免在真实流量样本与私有协议细节未验证前改动转发路径。
- 接入前置条件：真实请求 / 响应抓包、流式差分全覆盖、错误体全量映射、私有协议的鉴权 / header / URL 由 egress 层负责（不属于本 crate）。
- 接入时另立 change + ADR：`ProviderEntry` 增加 `upstream_protocol`；同协议保持字节直通，不同协议才调用 `direction()` / `stream_translator()`。
- 当前已知差距：真实流量抓包与私有协议 header / auth 尚未纳入；流式差分覆盖 6 个方向（Chat→Anthropic 使用 sub2api 两跳路径作为参考）；`9.3` 用 no-panic 等价用例替代 proptest。

## 22. 与 sub2api 的功能对照（2026-10-23）

已补齐：

- reasoning 模型（`gpt-5+`）转换时剥离 `temperature`；
- Chat→Responses / Anthropic→Responses 的 `max_output_tokens` 下限 128；
- Anthropic `tool_result` 中的图片 / 文档提升为独立 user 消息（Chat / Responses）；
- `tool_choice` 引用未声明工具时丢弃并 warning；
- `thinking.type=disabled` 覆盖 `output_config.effort`；
- Chat→Anthropic 的 reasoning-only 响应回退为 text；
- Responses `developer` 消息合并进 Anthropic `system`；
- custom / grammar tools 在 Responses↔Chat / Responses↔Anthropic 之间降级为 function 工具；
- namespace 子工具展平（`namespace__name`）及冲突拒绝；
- `tool_search` 降级为同名 function 代理，回程还原为 `tool_search_call`；
- built-in tools：Chat↔Responses 的 `web_search` / `x_search` / `code_execution`，Anthropic↔Responses 的 `web_search`；
- JSON Schema 根 union（allOf / anyOf / oneOf）展平；
- 流式工具调用晚到 name、先到 arguments、空 arguments 占位 delta；
- Responses 自定义工具历史（custom_tool_call / custom_tool_call_output / tool_search_output）；
- Chat / Responses / Anthropic 响应缺失 id、空 choices 的合成；
- usage 的 audio / prediction token 明细与 refusal 字段保留；
- Responses→Anthropic 的 tool_use / tool_result 配对归一化；
- Anthropic system 的 `x-anthropic-billing-header:` 过滤；
- Responses `Read` 工具的空 `pages` 参数裁剪；
- Chat→Responses 回程的 namespace 还原（按 `namespace__name` 前缀启发式拆分）；
- Chat / Responses / Anthropic 响应的缺失 id、空 choices 合成；
- usage 的 audio / prediction token 明细与 refusal 字段保留；
- Chat reasoning 回放为 Responses 内联 `<thinking>`，与 sub2api 一致；
- Anthropic→Chat 的 thinking 只在 tool-call turn 回放、`max_completion_tokens` 下限 128、text-only user content 折叠为字符串、reasoning 模型默认 `reasoning_effort=medium`；
- Anthropic thinking 无 signature 时丢弃（Responses 方向）；
- Responses reasoning summary-only 丢弃（Anthropic 方向）；
- Responses→Chat 的 agent 消息转 user，无文本时跳过；
- Chat legacy function call / result 按 name 配对 call_id；
- 流式 Responses→Anthropic 保留 thinking signature（envelope 解码 + signature_delta）；
- zero token details 省略、x_search tool_choice 使用字符串形态；
- Chat→Responses 的 system / developer 作为 input message 保留角色，`instructions` 只取请求顶层字段；`stream=true`、`store=false`、`include=["reasoning.encrypted_content"]` 与 sub2api 一致；
- legacy Chat function_call / function 结果分配不冲突的 `call_legacy_N` 并成对；
- Responses→Anthropic 未知 item 类型保留可识别文本，不再直接丢弃；
- Chat→Responses 的 `stream=true` / `store=false` / `include=reasoning.encrypted_content`、system/developer 角色保留、legacy `call_legacy_N` 配对与 sub2api 一致；
- Responses→Chat 流式 `function_call_arguments.done` 无 delta 时补发完整参数；
- Chat→Responses 流式 custom 工具补发 `custom_tool_call_input.done`；
- `created` 只在首个 chunk 固定，空消息列表不再直接报错；
- Responses→Chat / Anthropic 从 `output_text.done` 与终止响应中恢复缺失文本；
- Responses→Chat 支持 tool_search discovery promotion 与 additional_tools 展平；
- thinking signature 使用原始 signature（兼容 sub2api），同时保留 envelope 解码兼容；
- Responses→Anthropic 的 agent 消息转 user；
- Chat→Anthropic 的 reasoning 改为内联 `<thinking>` 文本，避免无 signature 的 thinking block；
- Responses→Anthropic 默认 `max_tokens` 对齐为 8192；
- Responses→Chat / Responses→Anthropic 从 `output_text.done` 与终止响应恢复缺失文本；
- tool_search discovery promotion、additional_tools 展平；
- thinking signature 使用原始 signature，并保留 envelope 解码；
- web_search / x_search 的 citations / results 映射到 Anthropic citations / tool_result 与 Chat annotations / tool arguments；
- Chat annotations 与 Anthropic citations 双向保留；
- JSON Schema 支持本地 `$ref`、`additionalProperties` 合并，嵌套 union 保留；
- Anthropic text document 映射为 Chat text / Responses input_text；
- Chat / Responses `input_audio` 双向携带；
- `allowed_tools` tool_choice 双向透传；
- 错误体保留 `extra`，包括 provider code / param / retry_after；
- Codex 方言补 `stream=true`、`include=reasoning.encrypted_content`；
- Chat→Anthropic 流式工具参数改为按 `index` 先聚合、结束后按序输出，支持并行工具参数交错；
- Responses `web_search_call` / `x_search_call` 与 Anthropic `server_tool_use` / `web_search_tool_result` 在请求、响应、流式三条路径上完整成对映射，保留 `srvtoolu_` id、query action、results 与 citations；
- Responses text annotations ↔ Anthropic text citations 双向保留；
- JSON Schema 递归展开本地 `$ref`，检测引用环；`additionalProperties` 在多分支 anyOf 冲突时丢弃；
- Chat / Responses `input_audio` 增加 data/format 原生语义校验，不合法时不透传；
- Chat 工具调用缺 id 时生成 `toolu_N`；流式工具始终缺 name 时仍发空 name 并显式 warning；
- 六个方向的流式错误体同样保留 provider `code` / `param` / `retry_after`。

egress 层（独立 crate `solidway_egress`，不反向依赖方向模块）：

- Claude Code：OAuth Bearer / API key 二选一；`anthropic-version`、`anthropic-beta` 合并去重、X-Stainless / `x-app` / CLI User-Agent；`metadata.user_id` 按 CLI 版本生成 legacy 或 JSON 编码，并提供 `apply_metadata()` 写回 body。
- Codex：Bearer、`chatgpt-account-id`、`originator`、`version`、`OpenAI-Beta`、`session_id` / `conversation_id`、Codex User-Agent。
- 纯函数：不发起 HTTP、不刷新 token、不读环境变量；token 获取/账号选择仍在调用方 egress 运行时。

sub2api oracle 批量对照：

- `tests/reference/oracle/` 下按方向保存 15 个 oracle 产出。
- `tests/oracle_corpus.rs` 对全部用例比较文本/工具调用/工具结果语义；对 `chat_to_responses`（parallel）、`responses_to_chat`（parallel）与 `responses_to_anthropic`（multimodal）做规范化 JSON 全量对拍。
- `tests/sub2api_port.rs` 直接移植 49 组 sub2api apicompat 行为测试，覆盖请求 / 响应 / 流式、custom / namespace / tool_search、schema union、invalid blocks、usage、agent message、text recovery、tool pairing、stream lifecycle、discovery promotion、ambiguous name 等；移植过程中同步修正了 content 折叠、tool 参数按序分片、tool index 映射、system/developer、reasoning fallback、tool_choice 丢弃、orphan/unanswered tool 清理等差异。
- 其余用例的已知差异（thinking 保留策略、默认注入字段、`is_error:false`、图片 `detail`、文档/文件支持）由语义摘要隔离。

说明：

- 自定义工具 / namespace 的响应还原需要调用方通过 `response_context()` 从请求构建上下文，并调用 `response_with_context()` / `stream_translator_with_context()`；不带上下文时仍可转换，但 custom 工具会退化为普通 function_call。
- namespace 在无上下文时使用 `__` 前缀启发式还原；带上下文时精确还原。
