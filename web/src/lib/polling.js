/**
 * 控制台实时数据的唯一取数方式：带间隔的轮询。
 *
 * 设计约束（见 `openspec/specs/admin-console`「实时数据获取」）：控制台 MUST NOT
 * 依赖 SSE / WebSocket / 其它长连接推送通道，也 MUST NOT 消费数据面的
 * `text/event-stream` 端点。实时性一律由 react-query 的 `refetchInterval` 提供——
 * 同一套 `/api/*` JSON 接口既能被浏览器、也能被桌面 webview 与反向代理安全消费。
 */

/** 4xx 是确定性失败（会话过期、参数非法）：继续轮询只会重放同一份错误。 */
export function shouldKeepPolling(error) {
  const status = error?.status
  if (typeof status !== 'number') return true
  return status < 400 || status >= 500
}

/**
 * `refetchInterval` 包装：4xx 时停表，后续成功取数后由 react-query 重新求值并恢复。
 *
 * @param {number} intervalMs 正常刷新间隔（毫秒）
 */
export function pollEvery(intervalMs) {
  return (query) => (shouldKeepPolling(query.state.error) ? intervalMs : false)
}

/**
 * 有界顺序轮询：上一次任务结束后才排下一次，**不会重叠**；返回取消函数。
 *
 * 用于「等外部状态变化」的一次性等待（桌面壳回执、重启后服务恢复），
 * 而不是周期刷新数据。`task` 返回 `true` 表示达成；返回 `false` 或抛错表示继续。
 *
 * @param {(attempt: number) => Promise<boolean> | boolean} task
 * @param {{ intervalMs: number, attempts?: number, onDone?: (done: boolean) => void }} options
 *   `attempts` 缺省为无穷（直到 `task` 返回 `true` 或被取消）；
 *   `onDone` 在达成或被取消以外结束时调用一次（达成传 `true`，用尽次数传 `false`）。
 * @returns {() => void} 取消函数
 */
export function pollUntil(task, { intervalMs, attempts = Infinity, onDone } = {}) {
  let cancelled = false
  let timer = null
  let count = 0

  const finish = (done) => {
    if (cancelled) return
    cancelled = true
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
    onDone?.(done)
  }

  const tick = async () => {
    if (cancelled) return
    count += 1
    let done = false
    try {
      done = (await task(count)) === true
    } catch {
      done = false
    }
    if (cancelled) return
    if (done) {
      finish(true)
      return
    }
    if (count >= attempts) {
      finish(false)
      return
    }
    timer = setTimeout(tick, intervalMs)
  }

  // 首次也等一个间隔：调用点通常刚发过请求（或刚触发重启），立刻再发一次没有意义。
  timer = setTimeout(tick, intervalMs)
  return () => {
    cancelled = true
    if (timer !== null) {
      clearTimeout(timer)
      timer = null
    }
  }
}
