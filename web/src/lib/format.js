/** 把后端返回的 Unix 秒格式化为本地时间。 */
export function formatUnix(seconds) {
  if (!seconds) return '-'
  return new Date(seconds * 1000).toLocaleString()
}

const compactFormatter = new Intl.DateTimeFormat(undefined, {
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
})

/** 把后端返回的 Unix 秒格式化为紧凑的本地时间（精确到分钟）。 */
export function formatUnixCompact(seconds) {
  if (!seconds) return '-'
  return compactFormatter.format(new Date(seconds * 1000))
}

/**
 * 按稳定机器码查 `errors.<code>` i18n；无匹配返回 `fallback`。
 */
export function codeMessage(t, code, fallback) {
  if (code) {
    const key = `errors.${code}`
    const translated = t(key)
    if (translated !== key) return translated
  }
  return fallback
}

/**
 * 把 `ApiError` 映射为可读文案：优先按 `code` 查 i18n，其次回退 message。
 */
export function errorMessage(t, error) {
  return codeMessage(t, error?.code, error?.message) || t('errors.generic')
}
