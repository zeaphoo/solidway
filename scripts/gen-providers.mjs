#!/usr/bin/env node
/**
 * 生成 provider 目录：以 models.dev 为机械来源，叠加受版本控制的
 * 协议族 / 鉴权映射与 featured 覆盖表，产出内置的 providers.json。
 *
 * 用法:
 *   node scripts/gen-providers.mjs                 # 从 models.dev 拉取
 *   node scripts/gen-providers.mjs --from-file f   # 使用本地快照（离线/CI）
 *
 * 运行时（solidway）不依赖网络；本脚本仅在开发期运行。
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const SOURCE_URL = 'https://models.dev/api.json'
const OUT_PATH =
  process.env.PROVIDERS_OUT ??
  resolve(
    dirname(fileURLToPath(import.meta.url)),
    '../crates/solidway_api/src/provider/providers.json',
  )

/** npm 包 → auth_scheme；未命中回退默认。 */
const NPM_AUTH = {
  '@ai-sdk/openai': 'bearer',
  '@ai-sdk/openai-compatible': 'bearer',
  '@ai-sdk/azure': 'api-key',
  '@ai-sdk/anthropic': 'x-api-key',
  '@ai-sdk/google-vertex': 'bearer',
  '@ai-sdk/google-vertex/anthropic': 'x-api-key',
  '@ai-sdk/amazon-bedrock': 'sigv4',
  '@ai-sdk/mistral': 'bearer',
  '@ai-sdk/cohere': 'bearer',
  '@ai-sdk/groq': 'bearer',
  '@ai-sdk/cerebras': 'bearer',
  '@ai-sdk/deepseek': 'bearer',
  '@ai-sdk/xai': 'bearer',
  '@ai-sdk/togetherai': 'bearer',
  '@ai-sdk/deepinfra': 'bearer',
  '@ai-sdk/perplexity': 'bearer',
  '@ai-sdk/fireworks': 'bearer',
  '@openrouter/ai-sdk-provider': 'bearer',
  '@ai-sdk/gateway': 'bearer',
  '@ai-sdk/alibaba': 'bearer',
}

const DEFAULT_AUTH = 'bearer'

/** 未在 FEATURED 覆盖表中的中国主流服务商，仍归入“常用”。 */
const FEATURED_EXTRA = new Set([
  'zhipuai',
  'volcengine',
  'tencent-tokenhub',
  'xiaomi',
  'minimax-cn',
  'siliconflow-cn',
  'stepfun',
  'stepfun-ai',
  'sensenova',
  'longcat',
  'modelscope',
  'bailing',
])

/** 无法用三种入站格式直连的 npm。生成时直接剔除。 */
const INCOMPATIBLE_NPM = new Set([
  '@ai-sdk/google',
  '@ai-sdk/google-vertex',
  '@ai-sdk/amazon-bedrock',
  'gitlab-ai-provider',
])

/** npm → 入站格式；无法映射返回 null（不兼容）。 */
function inboundFormatsFor(npm) {
  if (INCOMPATIBLE_NPM.has(npm)) return null
  if (npm === '@ai-sdk/anthropic' || npm === '@ai-sdk/google-vertex/anthropic') {
    return ['anthropic_messages']
  }
  return ['chat_completions']
}

/** 预设类型覆盖（未覆盖时按 key / 地址自动分类）。 */
const KIND_OVERRIDES = {
  'github-copilot': 'subscription',
  opencode: 'subscription',
  'opencode-go': 'subscription',
  'cline-pass': 'subscription',
  llmgateway: 'subscription',
  'llmgateway-providers': 'subscription',
  poe: 'subscription',
  synthetic: 'subscription',
  'bedrock-anthropic': 'enterprise',
  'google-vertex-anthropic': 'enterprise',
  'google-vertex-global-anthropic': 'enterprise',
}

/** 特殊注意事项（i18n key）。 */
const PRESET_NOTES = {
  minimax: 'providers.presetNotes.minimaxAnthropic',
  'minimax-cn': 'providers.presetNotes.minimaxAnthropic',
  'minimax-coding-plan': 'providers.presetNotes.minimaxAnthropic',
  'minimax-cn-coding-plan': 'providers.presetNotes.minimaxAnthropic',
  thinkingmachines: 'providers.presetNotes.anthropicCompatible',
  freemodel: 'providers.presetNotes.anthropicCompatible',
  subconscious: 'providers.presetNotes.anthropicCompatible',
  'kimi-code-plan-global': 'providers.presetNotes.kimiCoding',
  'kimi-code-plan-cn': 'providers.presetNotes.kimiCoding',
  'github-copilot': 'providers.presetNotes.githubCopilot',
  opencode: 'providers.presetNotes.opencode',
  'opencode-go': 'providers.presetNotes.opencode',
  'cline-pass': 'providers.presetNotes.opencode',
  llmgateway: 'providers.presetNotes.opencode',
  'llmgateway-providers': 'providers.presetNotes.opencode',
  'bedrock-anthropic': 'providers.presetNotes.bedrock',
  'google-vertex-anthropic': 'providers.presetNotes.vertex',
  'google-vertex-global-anthropic': 'providers.presetNotes.vertex',
  poe: 'providers.presetNotes.poe',
}

function kindFor(key, name, url) {
  if (KIND_OVERRIDES[key]) return KIND_OVERRIDES[key]
  if (/token plan/i.test(name) || /token-plan/.test(key)) return 'token_plan'
  if (/coding plan|for coding|step plan/i.test(name) || /coding-plan|code-plan|step-plan/.test(key)) {
    return 'coding_plan'
  }
  if (/(^|\/)(localhost|127\.0\.0\.1|0\.0\.0\.0)(:|\/|$)/.test(url)) return 'local'
  return 'api'
}

/**
 * featured provider 的权威覆盖。存在即标记 featured；
 * 字段缺省时沿用 models.dev 的值（如 openrouter/deepseek 已带 api）。
 * base_url 中的 {region} / {resource} 为占位符，由使用方替换。
 */
const PROJECT_VAR = {
  name: 'project',
  label: 'providers.params.project',
  kind: 'gcp-project',
  example: 'my-project',
  required: true,
}

const LOCATION_VAR = {
  name: 'location',
  label: 'providers.vars.location',
  kind: 'gcp-location',
  example: 'us-east5',
  required: true,
}

/** 预设中文显示名；专有名词可与英文同名。 */
const PRESET_NAME_ZH = {
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  'openai-compatible': 'OpenAI 兼容',
  'google-openai': 'Google Gemini（OpenAI 兼容）',
  azure: 'Azure',
  'azure-v1': 'Azure（v1 API）',
  'bedrock-anthropic': 'Bedrock（Anthropic）',
  'google-vertex-anthropic': 'Vertex（Anthropic）',
  'google-vertex-global-anthropic': 'Vertex（全球，Anthropic）',
  alibaba: '阿里云',
  bailing: '蚂蚁百灵',
  baichuan: '百川智能',
  hunyuan: '腾讯混元',
  lingyiwanwu: '零一万物',
  longcat: 'LongCat（美团）',
  modelscope: '魔搭 ModelScope',
  qianfan: '百度智能云千帆',
  sensenova: '商汤日日新',
  siliconflow: '硅基流动',
  'siliconflow-cn': '硅基流动（中国）',
  stepfun: '阶跃星辰（中国）',
  'stepfun-ai': '阶跃星辰（国际）',
  'tencent-tokenhub': '腾讯 TokenHub',
  volcengine: '火山引擎方舟',
  xfyun: '讯飞星火',
  xiaomi: '小米',
  'alibaba-cn': '阿里云（中国）',
  'alibaba-coding-plan': '阿里云编程套餐',
  'alibaba-coding-plan-cn': '阿里云编程套餐（中国）',
  'alibaba-token-plan': '阿里云 Token 套餐',
  'alibaba-token-plan-cn': '阿里云 Token 套餐（中国）',
  zhipuai: '智谱 AI',
  'zhipuai-coding-plan': '智谱 AI 编程套餐',
  zai: 'Z.AI',
  'zai-coding-plan': 'Z.AI 编程套餐',
  moonshotai: '月之暗面',
  'moonshotai-cn': '月之暗面（中国）',
  'kimi-code-plan-global': 'Kimi For Coding（国际）',
  'kimi-code-plan-cn': 'Kimi For Coding（中国）',
  minimax: 'MiniMax（国际）',
  'minimax-cn': 'MiniMax（中国）',
  'minimax-coding-plan': 'MiniMax Token 套餐（国际）',
  'minimax-cn-coding-plan': 'MiniMax Token 套餐（中国）',
  'tencent-coding-plan': '腾讯编程套餐（中国）',
  'tencent-token-plan': '腾讯 Token 套餐',
  'tencent-tokenhub': '腾讯 TokenHub',
  'xiaomi-token-plan-cn': '小米 Token 套餐（中国）',
  'xiaomi-token-plan-ams': '小米 Token 套餐（欧洲）',
  'xiaomi-token-plan-sgp': '小米 Token 套餐（新加坡）',
  'volcengine-coding-plan': '火山引擎方舟编程套餐',
  'stepfun-step-plan': '阶跃星辰 Step 套餐（中国）',
  'stepfun-ai-step-plan': '阶跃星辰 Step 套餐（国际）',
  'scnet-token-plan': 'SCNet Token 套餐',
  'kuae-cloud-coding-plan': 'KUAE 云编程套餐',
  'umans-ai-coding-plan': 'Umans AI 编程套餐',
  llmgateway: 'DevPass（LLM Gateway）',
  'llmgateway-providers': 'LLM Gateway',
  'vercel': 'Vercel AI 网关',
  nvidia: 'NVIDIA',
  deepinfra: 'DeepInfra',
}

function nameZhFor(key, name) {
  if (PRESET_NAME_ZH[key]) return PRESET_NAME_ZH[key]
  return name
    .replace(/ \(China\)$/, '（中国）')
    .replace(/ \(Global\)$/, '（国际）')
    .replace(/ \(Europe\)$/, '（欧洲）')
    .replace(/ \(Singapore\)$/, '（新加坡）')
    .replace(/ Coding Plan$/, '编程套餐')
    .replace(/ Token Plan$/, ' Token 套餐')
    .replace(/ Step Plan$/, ' Step 套餐')
}

/** models.dev 的 `${...}` 占位符 → 受版本控制的变量声明。 */
const BASE_URL_VARIABLES = {
  'cloudflare-workers-ai': [
    { name: 'CLOUDFLARE_ACCOUNT_ID', label: 'providers.vars.accountId', kind: 'path-segment', example: '0123456789abcdef', required: true },
  ],
  databricks: [
    { name: 'DATABRICKS_HOST', label: 'providers.vars.workspaceHost', kind: 'host-only', example: 'dbc-1234abcd-5678.cloud.databricks.com', required: true },
  ],
  infomaniak: [
    { name: 'INFOMANIAK_PRODUCT_ID', label: 'providers.vars.productId', kind: 'path-segment', example: '12345', required: true },
  ],
  neon: [
    { name: 'NEON_AI_GATEWAY_BASE_URL', label: 'providers.vars.gatewayBaseUrl', kind: 'full-url', example: 'https://ep-cool-123456.us-east-2.aws.neon.tech', required: true },
  ],
  'snowflake-cortex': [
    { name: 'SNOWFLAKE_ACCOUNT', label: 'providers.vars.snowflakeAccount', kind: 'host-label', example: 'myorg-myaccount', required: true },
  ],
}

const FEATURED = {
  openai: { base_url: 'https://api.openai.com/v1', auth_scheme: 'bearer', default_api_formats: ['chat_completions', 'responses'] },
  anthropic: { base_url: 'https://api.anthropic.com/v1', auth_scheme: 'x-api-key' },
  'google-vertex-anthropic': {
    featured: false,
    base_url: 'https://{location}-aiplatform.googleapis.com',
    group: 'Vertex AI',
    variant_label: 'providers.variants.vertexAnthropic',
    auth_scheme: 'google_service_account',
    default_api_formats: ['anthropic_messages'],
    variables: [LOCATION_VAR],
    path_template: '/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{model}:{action}',
    path_variables: [PROJECT_VAR, LOCATION_VAR],
  },
  'google-vertex-global-anthropic': {
    name: 'Vertex (Global, Anthropic)',
    doc: 'https://cloud.google.com/vertex-ai/generative-ai/docs/partner-models/claude',
    featured: false,
    base_url: 'https://aiplatform.googleapis.com',
    group: 'Vertex AI',
    variant_label: 'providers.variants.vertexGlobalAnthropic',
    auth_scheme: 'google_service_account',
    default_api_formats: ['anthropic_messages'],
    path_template: '/v1/projects/{project}/locations/global/publishers/anthropic/models/{model}:{action}',
    path_variables: [PROJECT_VAR],
    variables: [],
  },
  azure: {
    base_url: 'https://{resource}.openai.azure.com',
    auth_scheme: 'api-key',
    variables: [{ name: 'resource', label: 'providers.vars.resource', kind: 'host-label', example: 'my-resource', required: true }],
    path_template: '/openai/deployments/{deployment}/{endpoint}?api-version={api_version}',
    path_variables: [
      { name: 'deployment', label: 'providers.params.deployment', kind: 'path-segment', example: 'gpt-4o-prod', required: true },
      { name: 'api_version', label: 'providers.params.apiVersion', kind: 'azure-api-version', example: '2024-10-21', required: true },
    ],
  },
  'azure-v1': {
    name: 'Azure (v1 API)',
    doc: 'https://learn.microsoft.com/en-us/azure/ai-foundry/openai/api-version-lifecycle',
    base_url: 'https://{resource}.openai.azure.com',
    auth_scheme: 'api-key',
    variables: [{ name: 'resource', label: 'providers.vars.resource', kind: 'host-label', example: 'my-resource', required: true }],
    path_template: '/openai/v1/{endpoint}',
    path_variables: [],
  },
  'bedrock-anthropic': {
    name: 'Bedrock (Anthropic)',
    doc: 'https://docs.aws.amazon.com/bedrock/latest/userguide/model-parameters-anthropic-claude-messages.html',
    featured: true,
    base_url: 'https://bedrock-runtime.{region}.amazonaws.com',
    auth_scheme: 'sigv4',
    default_api_formats: ['anthropic_messages'],
    variables: [{ name: 'region', label: 'providers.vars.region', kind: 'host-label', example: 'us-east-1', required: true }],
    path_template: '/model/{model}:{action}',
    path_actions: { unary: 'invoke', stream: 'invoke-with-response-stream' },
    aws_service: 'bedrock',
    anthropic_version: 'bedrock-2023-05-31',
  },
  openrouter: {},
  deepseek: {},
  xai: { base_url: 'https://api.x.ai/v1', auth_scheme: 'bearer' },
  groq: { base_url: 'https://api.groq.com/openai/v1', auth_scheme: 'bearer' },
  mistral: { base_url: 'https://api.mistral.ai/v1', auth_scheme: 'bearer' },
  cohere: { base_url: 'https://api.cohere.ai/compatibility/v1', auth_scheme: 'bearer' },
  perplexity: { base_url: 'https://api.perplexity.ai', auth_scheme: 'bearer' },
  togetherai: { base_url: 'https://api.together.xyz/v1', auth_scheme: 'bearer' },
  'fireworks-ai': {},
  cerebras: { base_url: 'https://api.cerebras.ai/v1', auth_scheme: 'bearer' },
  deepinfra: { base_url: 'https://api.deepinfra.com/v1/openai', auth_scheme: 'bearer' },
  nvidia: {},
  minimax: {},
  moonshotai: {},
  'moonshotai-cn': {},
  zai: {},
  'alibaba-cn': {},
  alibaba: {},
  siliconflow: {},
  vercel: { base_url: 'https://ai-gateway.vercel.sh/v1', auth_scheme: 'bearer' },
}

/** models.dev 未收录、但需要内置的条目。 */
const MANUAL = [
  {
    key: 'ollama',
    name: 'Ollama',
    base_url_template: 'http://localhost:11434/v1',
    auth_scheme: 'none',
    featured: true,
    doc: 'https://ollama.com',
    default_api_formats: ['chat_completions'],
    kind: 'local',
  },
  {
    key: 'openai-compatible',
    name: 'OpenAI Compatible',
    base_url_template: '${BASE_URL}',
    base_url_variables: [
      { name: 'BASE_URL', label: 'providers.vars.baseUrl', kind: 'full-url', example: 'https://proxy.example.com/v1', required: true },
    ],
    auth_scheme: 'bearer',
    featured: true,
    doc: null,
    default_api_formats: ['chat_completions'],
    kind: 'gateway',
  },
  {
    key: 'google-openai',
    name: 'Google Gemini (OpenAI Compatible)',
    base_url_template: 'https://generativelanguage.googleapis.com/v1beta/openai',
    auth_scheme: 'bearer',
    featured: true,
    doc: 'https://ai.google.dev/gemini-api/docs/openai',
    default_api_formats: ['chat_completions'],
    kind: 'api',
  },
  {
    key: 'qianfan',
    name: 'Baidu Qianfan',
    base_url_template: 'https://qianfan.baidubce.com/v2',
    auth_scheme: 'bearer',
    featured: true,
    doc: 'https://cloud.baidu.com/doc/WENXINWORKSHOP/s/Fm2vrveyu',
    default_api_formats: ['chat_completions'],
    kind: 'api',
  },
  {
    key: 'xfyun',
    name: 'iFlytek Spark',
    base_url_template: 'https://spark-api-open.xf-yun.com/v1',
    auth_scheme: 'bearer',
    featured: true,
    doc: 'https://www.xfyun.cn/doc/spark/HTTP%E8%B0%83%E7%94%A8%E6%96%87%E6%A1%A3.html',
    default_api_formats: ['chat_completions'],
    kind: 'api',
  },
  {
    key: 'hunyuan',
    name: 'Tencent Hunyuan',
    base_url_template: 'https://api.hunyuan.cloud.tencent.com/v1',
    auth_scheme: 'bearer',
    featured: true,
    doc: 'https://cloud.tencent.com/document/product/1729/111007',
    default_api_formats: ['chat_completions'],
    kind: 'api',
  },
  {
    key: 'baichuan',
    name: 'Baichuan AI',
    base_url_template: 'https://api.baichuan-ai.com/v1',
    auth_scheme: 'bearer',
    featured: true,
    doc: 'https://platform.baichuan-ai.com/docs/api',
    default_api_formats: ['chat_completions'],
    kind: 'api',
  },
  {
    key: 'lingyiwanwu',
    name: '01.AI',
    base_url_template: 'https://api.lingyiwanwu.com/v1',
    auth_scheme: 'bearer',
    featured: true,
    doc: 'https://platform.lingyiwanwu.com/docs',
    default_api_formats: ['chat_completions'],
    kind: 'api',
  },
]

function authFor(npm) {
  if (!npm) return DEFAULT_AUTH
  for (const [pkg, value] of Object.entries(NPM_AUTH)) {
    if (npm === pkg || npm.startsWith(`${pkg}/`)) return value
  }
  return DEFAULT_AUTH
}

function normalize(key, raw, extra = {}, featured = false) {
  const auth = authFor(raw?.npm)
  const override = extra
  const name = raw?.name ?? override.name ?? key
  const rawUrl = override.base_url ?? raw?.api ?? ''
  const formats = override.default_api_formats ?? inboundFormatsFor(raw?.npm)
  if (!formats || formats.length === 0) return null
  const isTemplate = /\{[^}]*\}/.test(rawUrl)
  const item = {
    key,
    name,
    name_zh: nameZhFor(key, name),
    ...(override.group ? { group: override.group } : {}),
    ...(override.variant_label ? { variant_label: override.variant_label } : {}),
    // 端点依部署而异的条目以单变量模板表达，不再留空。
    base_url_template: rawUrl === '' ? '${BASE_URL}' : rawUrl,
    base_url_variables:
      override.variables ??
      BASE_URL_VARIABLES[key] ??
      (rawUrl === ''
        ? [{ name: 'BASE_URL', label: 'providers.vars.baseUrl', kind: 'full-url', example: 'https://proxy.example.com/v1', required: true }]
        : isTemplate
          ? []
          : []),
    auth_scheme: override.auth_scheme ?? auth,
    featured: override.featured ?? featured,
    doc: raw?.doc ?? override.doc ?? null,
    default_api_formats: formats,
    kind: override.kind ?? kindFor(key, name, rawUrl),
  }
  const note = override.note ?? PRESET_NOTES[key]
  if (note) item.note = note
  if (override.path_template) item.path_template = override.path_template
  if (override.path_variables) item.path_variables = override.path_variables
  if (override.path_actions) item.path_actions = override.path_actions
  if (override.aws_service) item.aws_service = override.aws_service
  if (override.anthropic_version) item.anthropic_version = override.anthropic_version
  return item
}

async function loadSource(fromFile) {
  if (fromFile) {
    return JSON.parse(readFileSync(resolve(fromFile), 'utf8'))
  }
  const res = await fetch(SOURCE_URL)
  if (!res.ok) throw new Error(`拉取 ${SOURCE_URL} 失败: HTTP ${res.status}`)
  return res.json()
}

function parseArgs(argv) {
  const idx = argv.indexOf('--from-file')
  return idx >= 0 ? argv[idx + 1] : null
}

async function main() {
  const source = await loadSource(parseArgs(process.argv.slice(2)))

  const byKey = new Map()
  // 这三个 key 属于已移除的 Gemini 原生预设；models.dev 若再次收录也不得生成。
  const SKIP_KEYS = new Set(['google', 'google-vertex', 'google-vertex-global', 'amazon-bedrock', 'gitlab'])

  for (const raw of Object.values(source)) {
    if (SKIP_KEYS.has(raw.id)) continue
    const featured =
      Object.prototype.hasOwnProperty.call(FEATURED, raw.id) || FEATURED_EXTRA.has(raw.id)
    const item = normalize(raw.id, raw, FEATURED[raw.id] ?? {}, featured)
    if (item) byKey.set(raw.id, item)
  }

  for (const [key, override] of Object.entries(FEATURED)) {
    if (!byKey.has(key)) {
      // models.dev 未收录的 featured 项（理论上少见），用覆盖字段补建。
      const item = normalize(key, { name: override.name, doc: override.doc }, override, true)
      if (item) byKey.set(key, item)
    }
  }

  for (const entry of MANUAL) {
    byKey.set(entry.key, {
      key: entry.key,
      name: entry.name,
      name_zh: entry.name_zh ?? nameZhFor(entry.key, entry.name),
      base_url_template: entry.base_url_template,
      base_url_variables: entry.base_url_variables ?? [],
        auth_scheme: entry.auth_scheme,
      featured: entry.featured,
      doc: entry.doc,
      kind: entry.kind ?? 'api',
      ...(entry.default_api_formats ? { default_api_formats: entry.default_api_formats } : {}),
      ...(entry.path_template ? { path_template: entry.path_template } : {}),
      ...(entry.path_variables ? { path_variables: entry.path_variables } : {}),
    })
  }

  // 编程套餐 / Token 套餐同样属于高频入口，统一放入“常用”。
  for (const provider of byKey.values()) {
    if (provider.kind === 'coding_plan' || provider.kind === 'token_plan') {
      provider.featured = true
    }
  }

  const providers = [...byKey.values()].sort((a, b) => {
    if (a.featured !== b.featured) return a.featured ? -1 : 1
    return a.name.localeCompare(b.name)
  })

  const payload = {
    source: 'models.dev',
    generated_at: new Date().toISOString(),
    service_account_host_patterns: ['{prefix}.googleapis.com'],
    providers,
  }

  writeFileSync(OUT_PATH, `${JSON.stringify(payload, null, 2)}\n`, 'utf8')
  const featured = providers.filter((p) => p.featured).length
  console.log(`已生成 ${OUT_PATH}`)
  console.log(`  providers=${providers.length} featured=${featured}`)
}

main().catch((err) => {
  console.error(err.message ?? err)
  process.exit(1)
})
