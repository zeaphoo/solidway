# 入站协议家族：按家族枚举端点，`base_url` 为上游 API 根

状态：accepted（2026-10-19，用户裁决）；Gemini 家族与 `provider_native` 部分已被 `docs/adr/0013-drop-gemini-inbound.md` 取代（2026-10-23）

## 背景

数据面最初只有 OpenAI 的两个入站端点（`/v1/chat/completions`、`/v1/responses`），厂商已经落地的三种协议家族里，Anthropic 与 Gemini 的原生账号无法进池：`ApiFormat` 里虽有 `anthropic_messages` / `gemini_generate_content`，但没有对应端点。

要把这两家接进来，必须先回答两个问题——它们都不在代码里能看出来，且答错会以两种方式返工：

1. **上游地址怎么拼**：网关的承诺是「保真转发，只改写鉴权头与目标地址」。目标地址由什么规则从入站 path 推导出来？
2. **哪些家族进得来**：`provider_native`（Bedrock、各种私有协议）的 URL 形状不可知。是给它开一条「透传」通路，还是不做？

## 决策

### 1. 按家族枚举端点，不做通配

入站家族与其家族根是**常量表**：

| 家族（`api_format`） | 家族根 | 端点 |
|---|---|---|
| `chat_completions` | `/v1` | `POST /v1/chat/completions` |
| `responses` | `/v1` | `POST /v1/responses` |
| `anthropic_messages` | `/v1` | `POST /v1/messages`、`POST /v1/messages/count_tokens` |
| `gemini_generate_content` | `/v1beta` | `POST /v1beta/models/{model}:generateContent`、`:streamGenerateContent`、`:countTokens` |

家族根与 `model` 来源只按上表取值，**不**由 `provider_keys.protocol`、`auth_scheme` 或请求内容推断。`provider_keys.protocol` 已降级为只读元数据。

### 2. 目标地址 = `base_url`（上游 API 根）+ 入站 path 去掉家族根

~~~txt
目标 = base_url(去尾斜杠) + (入站 path 去掉家族根) + ("?" + 清理后的 query)
~~~

对照：

| 入站 | base_url | 目标 |
|---|---|---|
| `POST /v1/chat/completions` | `https://api.deepseek.com` | `https://api.deepseek.com/chat/completions` |
| `POST /v1/chat/completions` | `https://open.bigmodel.cn/api/paas/v4` | `.../api/paas/v4/chat/completions` |
| `POST /v1/messages` | `https://api.anthropic.com/v1` | `https://api.anthropic.com/v1/messages` |
| `POST /v1beta/models/{m}:generateContent` | `https://generativelanguage.googleapis.com/v1beta` | `.../v1beta/models/{m}:generateContent` |

`base_url` 的语义由固定为「**上游的** API 根」，与入站的家族根**无关**（OpenAI 家族的入站根是 `/v1`，但 DeepSeek 的上游根是空）。因此：

- 网关 MUST NOT 再补版本段，也 MUST NOT 二次剥离；
- 预设 `anthropic` 固定为 `https://api.anthropic.com/v1`、`google` 固定为 `https://generativelanguage.googleapis.com/v1beta`（由覆盖表给出，不采用 models.dev 的 `api` 字段）；
- `base_url` path 缺版本段时不拒绝保存，只返回可读告警（不自动改写用户输入）。

### 3. `model` 来源与别名回写位置随家族

入站不读 `model` 的位置不是实现细节，它决定了模型校验、别名映射与粘性是否可用：

| 家族 | `model` 来源 | 别名回写点 |
|---|---|---|
| OpenAI / Anthropic | 请求体顶层 `model` | 顶层 `model`；Anthropic 流式回写在 `message_start.message.model` |
| Gemini | path 段 `{model}` | 只改**请求** path 段；响应 `modelVersion` 保留上游值（已知偏差） |

### 4. 只做可枚举家族，`provider_native` 不开入站通路

`provider_native` 在数据面只有两个作用：作为 egress 元数据（`auth_scheme` 决定鉴权头）、以及被探针跳过。它不提供入站端点，控制台的格式多选也不展示它（已存值保留并标注「已隐藏」）。

## 被否方案

- **家族绝对路径拼接**（`base_url` + 完整入站 path）：`base_url` 语义会按家族分叉；Anthropic 兼容代理 `https://proxy/anthropic/v1` 会拼出 `/v1/v1/messages`；且与 OpenAI 兼容上游的任意前缀（`/openai`、`/api/paas/v4`）冲突。
- **按家族硬编码完整目标 path**：同上，把「前缀由 `base_url` 决定」这一条自由度丢掉。
- **用 `protocol` 字段驱动拼接**：`protocol` 是录入期元数据、可由用户覆盖，运行期依赖它会让转发行为取决于历史字段值。
- **route 级 path 透传**（`/native/{*rest}` → `base_url` + rest）承载 `provider_native`：看起来一劳永逸，但
  1. 无法从请求中取 `model`（Gemini 在 path、Bedrock 在 path），模型校验 / 别名 / 用量归因全部退化；
  2. 必须要求路由内所有 provider 的 URL 形状严格一致，否则「换号」会把请求打到结构不同的上游；
  3. 把「网关知道端点」换成「客户端知道端点」，与「账号不可用时脚本零改动」的承诺冲突。
- **不启用入站家族、只支持 OpenAI 兼容上游**：Anthropic / Gemini 原生账号永远进不了池，等于放弃这两家的客户存量脚本。

## 后果

- 新增家族只需在常量表加行（加端点 + 家族根 + `model` 来源），不需要改地址推导逻辑。
- 预设 `base_url` 成为**行为相关的数据**：改动它等于改动转发地址，必须同步 `scripts/gen-providers.mjs` 的覆盖表与 `providers.json`。
- Anthropic 的 `count_tokens` / Gemini 的 `countTokens` 作为「非生成端点」：计入请求数，但不计 token、不计「用量缺失」（见 `openspec` 的 `gateway-core`）。
- Gemini 响应里的 `modelVersion` 不做回写：别名生效时客户端会看到上游 id，属可接受偏差。
- `provider_native` 的客户需求若将来出现，需要重新评估透传方案（`docs/future-roadmap.md` 已记录），且评估时**必须**先解决「换号时 URL 形状一致」与「`model` 从哪来」两件事。
