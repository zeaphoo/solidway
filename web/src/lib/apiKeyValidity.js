/** API Key 有效期相关的纯函数 / 常量（与组件文件分离，避免 Fast Refresh 告警）。 */

export const DEFAULT_VALIDITY = { mode: 'never', date: '' }

const SOON_SECONDS = 7 * 86_400

/** 转为 Unix 秒；never 或未填自定义日期时返回 null。 */
export function toExpiresAt(validity) {
  if (validity.mode === 'never') return null
  if (validity.mode === 'custom') {
    if (!validity.date) return null
    const timestamp = new Date(`${validity.date}T23:59:59`).getTime()
    return Number.isNaN(timestamp) ? null : Math.floor(timestamp / 1000)
  }
  return Math.floor(Date.now() / 1000) + Number(validity.mode) * 86_400
}

/** 自定义模式必须选择日期，否则不允许提交。 */
export function isValidityComplete(validity) {
  return validity.mode !== 'custom' || Boolean(validity.date)
}

/** 日期输入框的最小值（本地 ISO 日期）。 */
export function todayIsoDate() {
  return new Date().toISOString().slice(0, 10)
}

/** 到期状态：never / expired / soon（7 天内）/ valid。 */
export function expiryState(expiresAt) {
  if (!expiresAt) return 'never'
  const now = Math.floor(Date.now() / 1000)
  if (expiresAt <= now) return 'expired'
  if (expiresAt <= now + SOON_SECONDS) return 'soon'
  return 'valid'
}
