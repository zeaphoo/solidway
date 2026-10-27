/**
 * 默认头像的纯函数实现：确定性配色 + 显示名首字。
 *
 * 约束（见 `openspec/changes/2026-10-23-profile-identity-avatar/`）：
 * - 种子取 `user.id`：同一用户跨会话、跨设备同一头像；改昵称不改配色。
 * - 零素材、零网络、零依赖：只有纯 CSS 背景 + 文本。
 * - 白字与配色对比度全部 ≥ 5:1（只取 Tailwind 700/800 档；600 档的
 *   teal / sky / orange / amber 与白字对比不足 4.5:1）。
 */

/** 8 组双色渐变：`from` / `to` 与白字的对比度均 ≥ 5:1。 */
export const PALETTES = [
  { from: '#0f766e', to: '#047857' }, // teal → emerald
  { from: '#0369a1', to: '#1d4ed8' }, // sky → blue
  { from: '#4338ca', to: '#6d28d9' }, // indigo → violet
  { from: '#7e22ce', to: '#a21caf' }, // purple → fuchsia
  { from: '#be123c', to: '#be185d' }, // rose → pink
  { from: '#c2410c', to: '#b91c1c' }, // orange → red
  { from: '#0e7490', to: '#0f766e' }, // cyan → teal
  { from: '#334155', to: '#1e293b' }, // slate
]

/** CJK 统一表意文字（含扩展 A 与兼容区）。 */
const CJK_PATTERN = /[\u3400-\u4DBF\u4E00-\u9FFF\uF900-\uFAFF]/

/** FNV-1a 32 位哈希：稳定、无依赖、对短字符串分布均匀。 */
export function hashSeed(seed) {
  const text = String(seed ?? '')
  let hash = 0x811c9dc5
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return hash >>> 0
}

/** 按种子选一组配色。 */
export function paletteFor(seed) {
  return PALETTES[hashSeed(seed) % PALETTES.length]
}

/**
 * 头像首字。
 *
 * - 取显示名（为空回落登录名）的首个非空白字符，按码点切分（emoji 不被截断）。
 * - 首字符是 CJK → 直接用这个字。
 * - 否则按 `[\s._-]+` 切段：≥2 段取前两段首字母；1 段取前两个字符；统一大写。
 * - 空值 → `?`。
 */
export function initialsFor(displayName, username) {
  const source = String(displayName ?? '').trim() || String(username ?? '').trim()
  if (!source) return '?'

  const first = Array.from(source)[0]
  if (CJK_PATTERN.test(first)) return first

  const parts = source.split(/[\s._-]+/).filter(Boolean)
  if (parts.length >= 2) {
    return (Array.from(parts[0])[0] + Array.from(parts[1])[0]).toUpperCase()
  }
  return Array.from(parts[0] ?? source)
    .slice(0, 2)
    .join('')
    .toUpperCase()
}

/** 头像背景：左上柔光 + 135° 双色渐变（比纯色更有层次，且只用 CSS）。 */
export function avatarBackground(seed) {
  const { from, to } = paletteFor(seed)
  return [
    'radial-gradient(circle at 30% 25%, rgba(255, 255, 255, 0.20), transparent 60%)',
    `linear-gradient(135deg, ${from} 0%, ${to} 100%)`,
  ].join(', ')
}
