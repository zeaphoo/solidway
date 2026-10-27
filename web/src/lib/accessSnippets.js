/** 接入示例生成：SDK 调用（按入站格式）与终端 Agent 配置（按 harness）。 */

export const FORMATS = ['chat_completions', 'responses', 'anthropic_messages']

export const ENDPOINTS = {
  chat_completions: 'POST /v1/chat/completions',
  responses: 'POST /v1/responses',
  anthropic_messages: 'POST /v1/messages',
}

export const SDK_TABS = ['curl', 'python', 'node']
export const SDK_LANGUAGE = { curl: 'bash', python: 'python', node: 'javascript' }

/** 终端 Agent：key / 需要的入站格式 / 代码语言。 */
export const HARNESSES = [
  { key: 'claude-code', language: 'bash', format: 'anthropic_messages' },
  { key: 'codex', language: 'ini', format: 'responses' },
  { key: 'opencode', language: 'json', format: 'chat_completions' },
  { key: 'pi', language: 'json', format: 'chat_completions' },
  { key: 'generic', language: 'bash', format: 'chat_completions' },
]

/** Anthropic SDK / Claude Code 自行拼 `/v1/messages`，base 用去掉 `/v1` 的根。 */
export function anthropicRoot(baseUrl) {
  return baseUrl.replace(/\/v1$/, '')
}

/** Codex 的 wire_api 二选一：有 responses 就用它，否则 chat。 */
export function harnessFormat(key, formats) {
  const harness = HARNESSES.find((item) => item.key === key)
  if (!harness) return null
  if (harness.key === 'codex') {
    return formats.includes('responses') ? 'responses' : 'chat_completions'
  }
  return harness.format
}

/** 生成 SDK 调用示例（curl / python / node）。 */
export function buildSdkSnippet(baseUrl, model, format, tab) {
  const modelLiteral = JSON.stringify(model)

  if (format === 'anthropic_messages') {
    const snippets = {
      curl: `curl ${baseUrl}/messages \\
  -H "x-api-key: <YOUR_API_KEY>" \\
  -H "anthropic-version: 2023-06-01" \\
  -H "Content-Type: application/json" \\
  -d '{"model":${modelLiteral},"max_tokens":1024,"messages":[{"role":"user","content":"Hello"}]}'`,
      python: `# 实际请求：POST ${baseUrl}/messages
from anthropic import Anthropic

client = Anthropic(
    base_url="${anthropicRoot(baseUrl)}",  # SDK 自行拼 /v1/messages
    api_key="<YOUR_API_KEY>",
)

resp = client.messages.create(
    model=${modelLiteral},
    max_tokens=1024,
    messages=[{"role": "user", "content": "Hello"}],
)
print(resp.content[0].text)`,
      node: `// 实际请求：POST ${baseUrl}/messages
import Anthropic from "@anthropic-ai/sdk"

const client = new Anthropic({
  baseURL: "${anthropicRoot(baseUrl)}", // SDK 自行拼 /v1/messages
  apiKey: "<YOUR_API_KEY>",
})

const resp = await client.messages.create({
  model: ${modelLiteral},
  max_tokens: 1024,
  messages: [{ role: "user", content: "Hello" }],
})
console.log(resp.content[0].text)`,
    }
    return snippets[tab]
  }

  if (format === 'responses') {
    const snippets = {
      curl: `curl ${baseUrl}/responses \\
  -H "Authorization: Bearer <YOUR_API_KEY>" \\
  -H "Content-Type: application/json" \\
  -d '{"model":${modelLiteral},"input":"Hello"}'`,
      python: `# 实际请求：POST ${baseUrl}/responses
from openai import OpenAI

client = OpenAI(
    base_url="${baseUrl}",
    api_key="<YOUR_API_KEY>",
)

resp = client.responses.create(
    model=${modelLiteral},
    input="Hello",
)
print(resp.output_text)`,
      node: `// 实际请求：POST ${baseUrl}/responses
import OpenAI from "openai"

const client = new OpenAI({
  baseURL: "${baseUrl}",
  apiKey: "<YOUR_API_KEY>",
})

const resp = await client.responses.create({
  model: ${modelLiteral},
  input: "Hello",
})
console.log(resp.output_text)`,
    }
    return snippets[tab]
  }

  const snippets = {
    curl: `curl ${baseUrl}/chat/completions \\
  -H "Authorization: Bearer <YOUR_API_KEY>" \\
  -H "Content-Type: application/json" \\
  -d '{"model":${modelLiteral},"messages":[{"role":"user","content":"Hello"}]}'`,
    python: `# 实际请求：POST ${baseUrl}/chat/completions
from openai import OpenAI

client = OpenAI(
    base_url="${baseUrl}",
    api_key="<YOUR_API_KEY>",
)

resp = client.chat.completions.create(
    model=${modelLiteral},
    messages=[{"role": "user", "content": "Hello"}],
)
print(resp.choices[0].message.content)`,
    node: `// 实际请求：POST ${baseUrl}/chat/completions
import OpenAI from "openai"

const client = new OpenAI({
  baseURL: "${baseUrl}",
  apiKey: "<YOUR_API_KEY>",
})

const resp = await client.chat.completions.create({
  model: ${modelLiteral},
  messages: [{ role: "user", content: "Hello" }],
})
console.log(resp.choices[0].message.content)`,
  }
  return snippets[tab]
}

/** 生成终端 Agent 配置片段。 */
export function buildHarnessSnippet(key, baseUrl, model, formats) {
  const hasResponses = formats.includes('responses')
  const hasChat = formats.includes('chat_completions')

  switch (key) {
    case 'claude-code':
      return `# Claude Code 使用 Anthropic Messages 协议
# 实际请求：POST ${baseUrl}/messages
export ANTHROPIC_BASE_URL="${anthropicRoot(baseUrl)}"
export ANTHROPIC_AUTH_TOKEN="<YOUR_API_KEY>"
export ANTHROPIC_MODEL="${model}"
claude`
    case 'codex': {
      const wire = hasResponses ? 'responses' : 'chat'
      return `# ~/.codex/config.toml
model = "${model}"
model_provider = "solidway"

[model_providers.solidway]
name = "SolidWay"
base_url = "${baseUrl}"
env_key = "SOLIDWAY_API_KEY"
wire_api = "${wire}"   # 该路由可用格式：${hasResponses ? 'responses' : 'chat_completions'}

# 实际请求：POST ${baseUrl}/${wire === 'responses' ? 'responses' : 'chat/completions'}
# 终端里：
# export SOLIDWAY_API_KEY="<YOUR_API_KEY>"
# codex`
    }
    case 'opencode':
      return `// opencode.json（或 ~/.config/opencode/opencode.json）
{
  "$schema": "https://opencode.ai/config.json",
  "provider": {
    "solidway": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "SolidWay",
      "options": {
        "baseURL": "${baseUrl}",
        "apiKey": "<YOUR_API_KEY>"
      },
      "models": {
        "${model}": { "name": "${model}" }
      }
    }
  }
}`
    case 'pi':
      return `// ~/.pi/agent/models.json
{
  "providers": {
    "solidway": {
      "baseUrl": "${baseUrl}",
      "api": "openai-completions",
      "apiKey": "<YOUR_API_KEY>",
      "models": [{ "id": "${model}" }]
    }
  }
}`
    default:
      return hasChat
        ? `# 任何 OpenAI 兼容的 CLI / SDK
export OPENAI_BASE_URL="${baseUrl}"
export OPENAI_API_KEY="<YOUR_API_KEY>"   # 网关接入凭据
# 模型：${model}`
        : `# 该路由没有 Chat Completions 格式；可用端点见上方「可用入站端点」`
  }
}

/**
 * 把 `/api/access` 的 `public_origins` 归一化为接入地址列表。
 *
 * 统一补 `/v1`、去尾斜杠、去重；未配置时回落 `fallbackOrigin`（当前访问地址）。
 * 帮助页、引导页与路由详情页 MUST 共用此函数，避免三处地址不一致。
 */
export function buildBaseUrls(origins, fallbackOrigin) {
  const normalize = (value) => {
    const trimmed = String(value ?? '')
      .trim()
      .replace(/\/+$/, '')
    if (!trimmed) return ''
    return trimmed.endsWith('/v1') ? trimmed : `${trimmed}/v1`
  }

  const configured = [...new Set((origins ?? []).map(normalize).filter(Boolean))]
  if (configured.length) return configured

  const fallback = normalize(fallbackOrigin)
  return fallback ? [fallback] : []
}

/**
 * 按启用中的 Provider 凭据推导可用入站格式与各格式下的模型清单。
 *
 * 顺序固定为 Chat Completions → Responses → Anthropic（与 `FORMATS` 一致），
 * 供路由详情页、帮助页与引导页共用。
 */
export function formatsAndModels(providerKeys) {
  const models = {
    chat_completions: new Set(),
    responses: new Set(),
    anthropic_messages: new Set(),
  }
  const formats = new Set()

  for (const row of providerKeys ?? []) {
    if (row?.status !== 'active') continue
    const inbound = (row.api_formats ?? []).filter((format) => format in models)
    if (!inbound.length) continue
    inbound.forEach((format) => formats.add(format))
    for (const mapping of row.models ?? []) {
      if (!mapping?.name) continue
      inbound.forEach((format) => models[format].add(mapping.name))
    }
  }

  return {
    availableFormats: FORMATS.filter((format) => formats.has(format)),
    modelsByFormat: Object.fromEntries(
      FORMATS.map((format) => [
        format,
        [...models[format]].sort((a, b) => a.localeCompare(b)),
      ]),
    ),
  }
}
