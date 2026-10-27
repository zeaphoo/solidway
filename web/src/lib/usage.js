/** 用量统计共用参数与格式化。 */

export const USAGE_RANGES = [
  '24h',
  'today',
  'yesterday',
  '7d',
  '30d',
  'this_month',
  'last_month',
  'custom',
]

export function dateInputToEpoch(value, offsetMinutes, endOfDay = false) {
  if (!value) return undefined
  const parts = value.split('-').map(Number)
  if (parts.length !== 3 || parts.some((part) => !Number.isFinite(part))) return undefined
  const [year, month, day] = parts
  const utcMidnight = Math.floor(Date.UTC(year, month - 1, day) / 1000)
  const offset = offsetMinutes ?? -new Date().getTimezoneOffset()
  const seconds = utcMidnight - offset * 60
  return endOfDay ? seconds + 86_400 : seconds
}

export function buildUsageParams({ range, from, to, ...rest }) {
  const params = {
    range,
    ...rest,
  }
  if (range === 'custom') {
    params.from = from
    params.to = to
  }
  return params
}

export function totalTokens(row) {
  return (row?.input_tokens ?? 0) + (row?.output_tokens ?? 0)
}

export function formatTokens(value) {
  const number = Number(value ?? 0)
  if (number >= 1_000_000_000) return `${(number / 1_000_000_000).toFixed(2)}B`
  if (number >= 1_000_000) return `${(number / 1_000_000).toFixed(2)}M`
  if (number >= 1_000) return `${(number / 1_000).toFixed(1)}K`
  return String(number)
}

export function formatPercent(value) {
  return `${((Number(value) || 0) * 100).toFixed(1)}%`
}

export function formatMs(value) {
  return `${Math.round(Number(value) || 0)} ms`
}
