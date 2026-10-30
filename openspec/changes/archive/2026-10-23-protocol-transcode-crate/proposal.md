# protocol-transcode-crate

## Why

当前网关是保真转发：入站格式必须与 Provider 声明的 `api_formats` 精确匹配（ADR 0003 / 0013），不符合三种公开格式的上游（Codex、Claude Code、Gemini CLI、Antigravity、Kimi、Grok 等 agent 原生协议；Bedrock Converse、Vertex 等私有形状）无法进池。

在接入数据面之前，先用一个**独立、可测试、零 IO** 的库把「私有协议 ↔ OpenAI Chat Completions / Responses / Anthropic Messages，以及三种公开格式两两互转」的可行性、语义损失边界与流式状态机验证清楚。本 change 只是一次 spike，**不接入 `solidway_core`**。

## What Changes

1. 新增库 crate `crates/solidway_transcode`（package `solidway-transcode`），纯函数、无 async runtime、无 HTTP、无 DB。
2. **方向独立（方案 B）**：每个「客户端格式 → 上游协议」组合是一个独立方向模块，请求、非流式响应、流式事件全部由该模块自己实现；方向之间零引用。
3. 实现三种公开格式的 6 个方向：`chat_to_anthropic` / `anthropic_to_chat` / `chat_to_responses` / `responses_to_chat` / `anthropic_to_responses` / `responses_to_anthropic`。
4. 每个方向声明自己的**方向级能力矩阵**（`DirectionCaps`）与必保语义；转换返回 `Report`，`Strict` / `BestEffort` 两种策略，禁止静默丢字段。
5. 私有协议优先用**方言（Dialect = base wire + patch）**，由 registry 组合复用 base 方向；首批只做 2 个 pilot。结构完全不同的私有协议写自己的方向模块。
6. **机械公共层**只放分帧（SSE / AWS eventstream）、JSON Pointer、id 前缀、`Option` 算术；不放任何跨协议语义转换。
7. 建立 golden fixture + 隔离守卫 + 流式顺序 + 健壮性测试，全程本地、不联网。

不修改：`solidway_core`、`solidway_api`、`solidway` 二进制、数据库 schema、既有 OpenSpec specs、转发白名单。

## Impact

- **新增**：`crates/solidway_transcode/`（workspace 根已用 `crates/*` 成员通配，无需改 `Cargo.toml` 成员列表）。
- **依赖**：仅 `serde` / `serde_json` / `thiserror` / `bytes` / `indexmap` / `smallvec` / `tracing`（新增第三方依赖需按 AGENTS.md 确认后再落代码）。
- **行为**：无。没有 crate 引用它，数据面与界面的运行时行为不变。

## 范围外

- 协议无关的 canonical IR（已否决，理由见 `design.md` §2.3）。
- 与 `solidway_core` 的任何集成、`ProviderEntry` 字段变化、转发路径变化。
- 鉴权、URL 拼接、`base_url` / 端点模板、模型别名、选号、换号、重试、用量落库。
- `/v1/models`、文件上传、Batch、Realtime、Embeddings、Images、Audio 等非对话端点。
- token 估算（沿用项目「不做估算」原则）。
- 真实上游联调与抓包；fixture 全部为本地脱敏样例。

## 验证要求

- `cargo test -p solidway-transcode` 通过，且测试不发起网络请求。
- `cargo build --workspace` 通过（新 crate 不影响既有 crate）。
- 6 个方向 ×（请求 / 非流式 / 流式）均有自包含 golden fixture，`Strict` 与 `BestEffort` 行为均有断言。
- 方向级能力矩阵的每个单元格都有测试覆盖，`Unsupported` 不允许静默通过。
- `tests/isolation.rs` 通过：方向之间零引用，`common` 不出现协议语义。
- 删除任一方向目录后，其余方向的编译与测试仍通过。
- 设计评审确认「是否接入 core」的结论后再另立 change，本 change 不修改任何 spec。

## 参考与许可

- `bifrost`（Apache-2.0）：canonical hub 方案**不采用**，只参考流式 state machine 与事件组织。
- `sub2api`（LGPL-3.0）：采用其「成对直桥」思路，参考语义映射表与 fixture 覆盖面，**不复制代码**。
- 本项目只参考行为与设计，不引入上述项目的源码；如需引入 fixture，自行编写或使用 Apache-2.0 来源并保留 NOTICE。
