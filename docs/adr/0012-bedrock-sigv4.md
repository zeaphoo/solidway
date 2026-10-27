# ADR 0012 — Bedrock 接入：SigV4 手写 + 动作名数据化

## 状态

已接受（2026-10-22）

## 背景

内置目录一直有 Bedrock 预设，但 `sigv4` 只有枚举没有实现（数据面 `auth_headers` 对它是空实现），控制台又把该预设过滤掉，实际上不可用。Bedrock 只差两层能力：按请求签名，以及"模型在路径里"的 Anthropic 端点（`/model/{id}:invoke`）。

需要决策的是：范围（Claude only 还是全模型）、签名实现（手写还是引 crate）、以及 `{action}` 与正文规则如何不进代码分支。

## 决策

1. **范围只做 Claude（Anthropic）**：复用 `anthropic_messages` 内容格式；Bedrock 的 `invoke` 与 Vertex 的 `rawPredict` 是同一类"模型在路径里"的 Anthropic 端点。Converse（`/model/{id}/converse`）是独立内容格式 + 新入站形状，单独立项。
2. **手写 SigV4**：core 增加 `hmac`（`sha2` / `hex` 已有），实现 canonical request → string to sign → signing key；用 AWS 官方文档的示例向量做单测。否决 `aws-sigv4` crate：只需 4 步，且要与既有 header 白名单/改写管线贴合。
3. **动作名下沉到目录**：预设声明 `path_actions: {unary, stream}`，数据面不再按厂商判断；缺省仍为 `rawPredict` / `streamRawPredict`。否决按 `aws_service` 特判。
4. **正文规则改为"模板事实"**：Anthropic 入站且模板含 `{model}` → 删顶层 `model` + 注入 `anthropic_version`。`anthropic_version` 由预设声明（Bedrock `bedrock-2023-05-31`、Vertex `vertex-2023-10-16`），数据面不硬编码。
5. **区域单一来源**：`region` 仍是 URL 变量（`bedrock-runtime.{region}.amazonaws.com`）；签名取 `options.region`，缺省回退 `options.base_url_vars.region`。否决从 host 解析（自建代理/VPC endpoint 会取错）。
6. **事件流按 content-type 识别**：`application/vnd.amazon.eventstream` 走流式透传、不解析 usage（记"用量缺失"）；请求体 `stream = true` 也标记为流式。
7. **能力边界**：`aws_sigv4` 与 Service Account 一样不可探测、不列模型（`unsupported_credential_kind`，自动拉取跳过）。

## 后果

- 目录：`amazon-bedrock` 更名为 `bedrock-anthropic`（只覆盖 Anthropic 模型），新增 `path_actions` / `aws_service` / `anthropic_version` 三个字段。
- 凭据：新增 `credential_kind = aws_sigv4`（JSON：access key / secret / 可选 session token），掩码 `aws:<后4位>`；DB `CHECK` 扩展。
- 数据面：`build_request` 与探针在最终请求体确定后签名；新增事件流分支。
- 遗留：Bedrock Converse 与非 Claude 模型（见 `docs/future-roadmap.md`）。
