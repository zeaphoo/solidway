# 移除 Gemini 入站通路与 provider_native 哨兵

状态：accepted（2026-10-23，用户裁决）

## 背景

对外入站原本有四种格式：Chat Completions、Responses、Anthropic Messages、Gemini generateContent。产品决定不再对外提供 Gemini 原生接口，只保留前三种。

网关不做格式转换，因此 `ApiFormat` 同时是“入站格式”和“Provider 可服务的格式”。Gemini 入站移除后，`gemini_generate_content` 不再对应任何已建模端点；`provider_native` 从未对应任何入站端点。两者继续留在取值域里会产生“Provider 可见但永不被选中”“模型出现在 `/v1/models` 但不可用”的死状态。

## 决策

1. **`ApiFormat` 收敛为三种**：`chat_completions` / `responses` / `anthropic_messages`。
2. **删除 `provider_native`**：用空 `api_formats` 表示“当前没有可服务的入站格式”。探针、不可探测、选号已对空集合有完整语义。
3. **删除 Gemini 端点与资源归属**：`/v1beta/*`、`/upload/*` 不再建模；路由挂载保留，统一返回 JSON 404，避免落入控制台 SPA fallback。
4. **删除 `x-goog-api-key`**：不再作为入站鉴权头，也不再作为 Provider 出站鉴权方式。转发前仍剥离该头，防止客户端凭据泄漏。
5. **删除 Gemini 原生预设**：`google`、`google-vertex`、`google-vertex-global` 移除；保留 `google-openai` 与 `google-vertex-*-anthropic`。
6. **`/v1/models` 只列出可服务模型**：模型并集只包含至少有一种入站格式可服务的模型；`models_set` 保留完整集合用于错误区分。
7. **不改数据库 schema、不做迁移**：`provider_resources` 表保留但不再读写；存量未知 `api_formats` / `x-goog-api-key` 记录不做兼容处理。

## 被否方案

- **只删入站、保留 `gemini_generate_content`**：Provider 永不被选中，但模型仍会出现在 `/v1/models`，需要额外过滤掩盖死状态。
- **保留 `provider_native` 作为“无格式”标记**：与空集合语义重复，且规格与代码原本就不一致。
- **删除 `/v1beta`、`/upload` 路由挂载**：请求会落入 SPA fallback 返回 200 HTML，破坏错误契约。
- **在 `api_formats` 中预留 `native` 供未来适配器使用**：维度错误。未来协议转换应作为独立的 egress 适配器维度，Provider 仍声明它能承接的三种入站格式之一。

## 后果

- 对外只保留三种入站格式。
- Google 模型需通过 `google-openai` 预设接入；Vertex Gemini 原生通路不再支持，Vertex Anthropic 不受影响。
- 未来若要做协议转换，需新增适配器设计（入站三格式 → 上游原生协议），不能复用 `api_formats` 作为协议类型。
- 相关旧规格（ADR 0003 的 Gemini 家族、ADR 0008/0010/0011 的 Gemini 分支）由本 ADR 取代其 Gemini 部分。
