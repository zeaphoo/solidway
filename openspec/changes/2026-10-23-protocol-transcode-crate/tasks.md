# Tasks — protocol-transcode-crate

> 对应提案与设计：`openspec/changes/2026-10-23-protocol-transcode-crate/`。
> 本 change 只做试验性 crate，不接入 `solidway_core`。
> 实现进度：6 个公开方向（请求 / 响应 / 流式）已完成；方言与私有 pilot 为下一步。
> 2026-10-23 可信度自查：逐方向补齐静默丢弃 warning、修正 caps、修流式多 choice / refusal / signature / ping 边界；新增 11 个方向级 warning 与流式边界测试。
> 2026-10-23 扩展验证：新增并行工具 / 多轮工具 / thinking + tool / 多模态 / 错误体 / 截断流 fixture；新增 sub2api 差分对照（6 个请求方向）。
> 2026-10-23 M3：方言 patch 引擎、codex / claude-code pilot、registry 组合、方言请求 / 响应 / 流式测试已完成。
> 2026-10-23 M4：no-panic 门槛、caps 矩阵、性能基准（ignored，release 已跑）、golden harness、流式差分（6 个方向）、接入结论已完成。
> 2026-10-23 继续对齐（八）：直接移植 49 组 sub2api apicompat 测试，修正 content/tool/system/reasoning/tool_choice 等差异，oracle corpus 扩到 3 个全量对拍。
> 2026-10-23 继续对齐（七）：新增 `solidway_egress`（OAuth/账号 metadata/beta header），sub2api oracle 15 用例批量 fixture + 2 个全量对拍。
> 2026-10-23 继续对齐（六）：流式工具聚合按序输出、server tool 完整成对映射、递归 $ref/引用环、audio 校验、缺 id 生成、name 缺失 warning。
> 2026-10-23 继续对齐（五）：流式文本恢复、tool_search discovery、additional_tools、citations/results、$ref/additionalProperties、document text、audio、allowed_tools、error extra、Codex stream/include。
> 2026-10-23 继续对齐（四）：流式文本恢复、tool_search discovery、additional_tools、signature 原始格式、agent 角色、默认 8192、Chat→Anthropic 内联 thinking。
> 2026-10-23 继续对齐（三）：Chat→Responses stream/store/include、system/developer、legacy ID；自定义工具流式 input done；function arguments done；未知 item 文本；空消息列表。
> 2026-10-23 继续对齐（二）：Chat→Responses 角色/stream/store/include、legacy function ID、未知 item 文本保留；
> 2026-10-23 继续对齐：thinking 落点/默认 effort、text-only user content 折叠、reasoning summary-only、agent 消息、legacy function 配对、流式 thinking signature、zero detail / x_search choice。
> 2026-10-23 sub2api 功能对照：补齐温度剥离、输出下限、tool_result 媒体、未声明 tool_choice、thinking disabled、reasoning-only fallback、developer→system、custom/namespace/tool_search/built-in tools、流式晚到 name / 空 args、id/空 choices、usage 明细与 refusal、billing header、Read 空 pages、namespace 还原、response context、server tool 形状、空 body 合成、Claude Code metadata 默认值；剩余项见 design §22。
> 方向语义：`src` = 客户端格式，`dst` = 上游协议；`request` 走 `src → dst`，`response` / `stream` 走 `dst → src`。

## 1. 骨架与边界

- [x] 1.1 新建 `crates/solidway_transcode/{Cargo.toml,src/lib.rs}`，package `solidway-transcode`，edition / license 继承 workspace
- [x] 1.2 依赖只允许 `serde` / `serde_json` / `thiserror` / `bytes` / `indexmap` / `smallvec` / `tracing`；新增依赖按 AGENTS.md 确认
- [x] 1.3 `lib.rs` 写明边界：无 tokio / axum / reqwest / sqlx / futures，无 IO、无全局可变状态
- [x] 1.4 确认 workspace `members = ["crates/*"]` 自动包含新 crate，`cargo build --workspace` 通过

## 2. 公共基础（只放机械工具）

- [x] 2.1 `protocol.rs`：`ProtocolId` / `WireKind` / `Support` / `DirectionCaps` / `RequestCaps` / `ResponseCaps` / `StreamCaps`
- [x] 2.2 `report.rs`：`Report` / `Warning` / `WarningCode` / `Side` / `TranscodePolicy`
- [x] 2.3 `common/frame.rs`：`FrameDecoder` / `FrameEncoder`（SSE 与 AWS eventstream），覆盖半帧、多行 data、`[DONE]`、截断
- [ ] 2.4 `common/json.rs` / `common/ids.rs` / `common/usage.rs`：JSON Pointer、id 前缀、`Option<u64>` 算术
- [x] 2.5 `common` 不得出现协议名、字段名或跨协议判断；单元测试覆盖上述工具

## 3. wire 类型

> wire = 某个协议的线上类型（serde 结构 / enum），字段与 JSON、SSE 事件 payload 一一对应。
> wire 不是 IR，不做规范化；三个 wire 之间不共享消息 / 内容块类型。
> "3 个 wire" 指 3 个公开协议的 wire，不是 6 个方向；方向模块按需组合其中两个。

- [x] 3.1 `wire/openai_chat.rs`：Chat Completions 的 `ChatRequest` / `ChatResponse` / `ChatChunk` / 错误外壳
- [x] 3.2 `wire/openai_responses.rs`：Responses 的 `ResponsesRequest` / `ResponsesResponse` / `ResponsesEvent` / 错误外壳
- [x] 3.3 `wire/anthropic.rs`：Anthropic Messages 的 `AnthropicRequest` / `AnthropicResponse` / `AnthropicEvent` / 错误外壳
- [x] 3.4 三个 wire 互相独立，不共享消息 / 内容块类型；serde roundtrip 与未知字段保留测试
- [x] 3.5 所有 wire 类型 `pub(crate)`，对外只暴露字节 API

## 4. 方向契约与 registry

- [x] 4.1 `Direction` trait / `StreamBridge` trait / `Plan` / `Transcoded`
- [x] 4.2 `registry.rs`：静态方向表 + `direction(src, dst)`
- [x] 4.3 `direction()` 未命中返回 `None`；`src == dst` 不处理，由调用方透传
- [x] 4.4 `Direction::caps()` 返回方向级能力矩阵

## 5. M0 样板方向 `chat_to_anthropic`

- [x] 5.1 `directions/chat_to_anthropic/mod.rs`：零大小 `Direction` 实现、caps、fidelity 常量
- [x] 5.2 `request.rs`：Chat → Anthropic（system / messages / tools / tool_choice / params / error）
- [x] 5.3 `response.rs`：Anthropic → Chat（content / finish_reason / usage / error）
- [x] 5.4 `stream.rs`：Anthropic 帧 → Chat 帧状态机
- [x] 5.5 `fidelity.rs`：`MUST_PRESERVE` / `KNOWN_LOSSY`
- [x] 5.6 `tests/golden/chat_to_anthropic/`：文本 / system / 工具 / 工具结果 / 图片 / thinking / usage / 错误 / 流式
- [x] 5.7 Strict / BestEffort 行为；流式顺序、终止唯一、截断、确定性
- [x] 5.8 评审样板目录，确认其余方向可按同一结构复制

## 6. 其余非流式方向（可并行）

- [x] 6.1 `anthropic_to_chat`：Anthropic → Chat 请求；Chat → Anthropic 响应
- [x] 6.2 `chat_to_responses`：Chat → Responses 请求；Responses → Chat 响应
- [x] 6.3 `responses_to_chat`：Responses → Chat 请求；Chat → Responses 响应
- [x] 6.4 `anthropic_to_responses`：Anthropic → Responses 请求；Responses → Anthropic 响应
- [x] 6.5 `responses_to_anthropic`：Responses → Anthropic 请求；Anthropic → Responses 响应
- [x] 6.6 每个方向包含 `mod.rs` / `request.rs` / `response.rs` / `fidelity.rs` + 自包含 fixture
- [x] 6.7 6 方向 caps 矩阵测试：`Unsupported` 在 Strict 报错、BestEffort 必须有 warning
- [x] 6.8 `tests/isolation.rs`：方向之间零引用

## 7. 流式方向（可并行）

- [x] 7.1 `anthropic_to_chat` 流式：Chat 帧 → Anthropic 帧
- [x] 7.2 `chat_to_responses` 流式：Responses 帧 → Chat 帧
- [x] 7.3 `responses_to_chat` 流式：Chat 帧 → Responses 帧
- [x] 7.4 `anthropic_to_responses` 流式：Responses 帧 → Anthropic 帧
- [x] 7.5 `responses_to_anthropic` 流式：Anthropic 帧 → Responses 帧
- [x] 7.6 每个流式方向覆盖：事件顺序、终止事件唯一、截断、未知事件、keepalive、确定性

## 8. 方言与私有 pilot

- [x] 8.1 `dialect/patch.rs`：`Rename` / `Wrap` / `Unwrap` / `Default` / `Set` / `Drop`
- [x] 8.2 `dialect/codex.rs`（base = `openai.responses`）
- [x] 8.3 `dialect/claude_code.rs`（base = `anthropic.messages`）
- [x] 8.4 `registry` 组合：`S→P` / `P→S` / `B→P` / `P→B` / `P1→P2`（同 base 与不同 base）
- [x] 8.5 组合方向 caps 逐项收窄（`Unsupported` > `Lossy` > `Emulated` > `Native`）
- [x] 8.6 私有协议脱敏 fixture（请求 / 响应 / 流式）
- [x] 8.7 `dialect/` 不得引用 `directions/`；组合逻辑只在 `registry`

## 9. 测试与收尾

- [x] 9.1 golden fixture harness：JSON 语义比较、SSE 逐帧比较
- [x] 9.2 conformance：caps × 方向（自洽 + 组合收窄）、`MUST_PRESERVE` 覆盖、`KNOWN_LOSSY` warning 覆盖
- [x] 9.3 `proptest` 或等价 no-panic 用例（不 panic、返回 `Error::Invalid`）
- [x] 9.4 确定性：同一输入重复转换，输出字节 / 帧序列一致
- [x] 9.5 隔离守卫：`tests/isolation.rs` + `scripts/check-transcode-isolation.sh`；删除任一方向目录验证其余方向仍通过
- [x] 9.6 `cargo test -p solidway-transcode` 与 `cargo build --workspace` 通过，测试不联网
- [x] 9.7 性能预算：10KB 请求转换 < 1ms（release；debug 放宽），记录基准
- [x] 9.8 设计复查：输出「是否接入 core」的结论；接入另立 change + ADR

## 10. 明确不做

- [x] 10.1 不引入协议无关 IR / canonical hub
- [x] 10.2 不修改 `solidway_core` / `solidway_api` / `solidway` 二进制
- [x] 10.3 不修改 `ApiFormat` / `ProviderEntry` / 转发白名单 / 数据库
- [x] 10.4 不修改 `openspec/specs/`（本 change 无 spec delta）
- [x] 10.5 不做 token 估算、`/v1/models`、文件上传、Batch、Realtime、Embeddings、Images、Audio 生成
