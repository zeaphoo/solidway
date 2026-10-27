/**
 * Provider 端点的纯逻辑：凭据类型识别、模板展开、预设反查与可探测判定。
 *
 * 与后端保持同一套规则，便于用 Node 内置 test runner 做表驱动验证
 * （见 `providerEndpoint.test.mjs`），组件只负责渲染。
 */

/** 凭据内容判定：SA JSON / AWS 凭据 JSON / 其余按 API Key。 */
export function detectCredentialKind(raw) {
  try {
    const parsed = JSON.parse(raw)
    if (parsed && parsed.type === 'service_account') return 'google_service_account'
    if (parsed?.access_key_id && parsed?.secret_access_key) return 'aws_sigv4'
  } catch {
    // 非 JSON 一律按 API Key 处理。
  }
  return 'api_key'
}

/** 凭据类型隐含的鉴权方式；`null` 表示由用户选择。 */
export function requiredAuthScheme(credentialKind) {
  if (credentialKind === 'google_service_account') return 'google_service_account'
  if (credentialKind === 'aws_sigv4') return 'sigv4'
  return null
}

/**
 * 模板展开；支持 `{name}` 与 `${NAME}`，先替换 `${NAME}` 以免残留 `$`。
 * 缺失变量按空串处理（保存前由表单校验拦截）。
 */
export function expandTemplate(template, vars) {
  let rendered = template ?? ''
  for (const [name, value] of Object.entries(vars ?? {})) {
    const text = String(value ?? '').trim()
    rendered = rendered.split(`\${${name}}`).join(text)
    rendered = rendered.split(`{${name}}`).join(text)
  }
  return rendered
}

/** 反查所属预设：精确匹配，否则剥离一次 `-custom` 后再匹配（不递归）。 */
export function presetOf(presets, provider) {
  const key = String(provider ?? '').trim()
  const exact = presets.find((preset) => preset.key === key)
  if (exact) return exact
  if (key.endsWith('-custom')) {
    const base = key.slice(0, -'-custom'.length)
    return presets.find((preset) => preset.key === base)
  }
  return undefined
}

/** 该条目是否为预设条目（端点等字段锁定）。 */
/**
 * 预设的本地化显示名：中文环境优先 `name_zh`，缺失或英文环境回退 `name`。
 */
export function presetDisplayName(preset, language) {
  if (!preset) return ''
  const zh = String(language ?? '').toLowerCase().startsWith('zh')
  return zh && preset.name_zh ? preset.name_zh : preset.name
}

export function isPresetEntry(presets, provider) {
  const key = String(provider ?? '').trim()
  return presets.some((preset) => preset.key === key)
}

/** 不可探测的原因 key（与后端 `probeable=false` 的三种成因对应）。 */
export function unprobeableReason(row) {
  if (row?.credential_kind === 'google_service_account') {
    return 'health.unprobeableServiceAccount'
  }
  if (!(row?.models ?? []).length) return 'health.unprobeableNoModels'
  return 'health.unprobeableNoFormats'
}

/** 健康列状态：cooling/retired 优先，其次不可探测，最后健康。 */
export function healthState(row, healthMark) {
  if (healthMark) return healthMark.state === 'retired' ? 'retired' : 'cooldown'
  if (row?.probeable === false) return 'unprobeable'
  return 'healthy'
}
