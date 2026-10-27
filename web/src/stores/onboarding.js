/**
 * 快速开始的「已跳过 / 已完成」标记（仅本机，按用户隔离）。
 *
 * 刻意不落库：它只记录主观意图（我不想看 / 我看完了），不承担进度判定——
 * 进度一律由接口数据推导（见 `lib/onboardingSteps.js`）。因此丢失无数据风险，
 * 换设备或清缓存最多再看一遍引导页。
 */

const KEY_PREFIX = 'solidway-onboarding:v1:'
const EMPTY = { dismissed: false, reason: null, at: null }

/** 按用户隔离的存储键；拿不到用户时用 `anonymous`，避免污染他人标记。 */
export function onboardingKey(userId) {
  return `${KEY_PREFIX}${userId ?? 'anonymous'}`
}

function storage() {
  try {
    return typeof window === 'undefined' ? null : window.localStorage
  } catch {
    // 隐私模式 / 禁用存储：按未标记处理，不抛错。
    return null
  }
}

/** 读取标记；键缺失、内容损坏、存储不可用一律返回未标记。 */
export function readOnboarding(userId) {
  const store = storage()
  if (!store) return { ...EMPTY }
  try {
    const raw = store.getItem(onboardingKey(userId))
    if (!raw) return { ...EMPTY }
    const parsed = JSON.parse(raw)
    return {
      dismissed: parsed?.dismissed === true,
      reason: typeof parsed?.reason === 'string' ? parsed.reason : null,
      at: Number.isFinite(parsed?.at) ? parsed.at : null,
    }
  } catch {
    return { ...EMPTY }
  }
}

/** 写入标记（跳过或全部完成）。`reason` 仅用于调试追溯。 */
export function markOnboardingDone(userId, reason = 'dismissed') {
  const store = storage()
  if (!store) return
  try {
    store.setItem(
      onboardingKey(userId),
      JSON.stringify({ dismissed: true, reason, at: Date.now() }),
    )
  } catch {
    // 配额满等写入失败：忽略，用户下次仍会看到引导，不影响功能。
  }
}

/** 清除标记（供「重新看一遍引导」之类的入口使用，当前未暴露）。 */
export function clearOnboarding(userId) {
  const store = storage()
  if (!store) return
  try {
    store.removeItem(onboardingKey(userId))
  } catch {
    // 同上：失败无副作用。
  }
}
