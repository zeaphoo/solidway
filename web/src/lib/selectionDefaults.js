/**
 * 选号策略表单的默认值推导。
 *
 * 必须与服务端同口径：
 * - `SelectionStrategy::default_weights` / `default_exploration_rate`
 * - `SelectionThresholds::default`
 * - `resolve()` = 策略默认值 ∪ 已保存的覆盖项
 *
 * 关键约束：切策略时要按**新策略**重算表单值。非打分策略（轮询 / 最后使用）
 * 不带权重，若沿用上一个策略的服务端 `resolved`，权重框会全是 0，
 * 用户一保存就会得到「权重全为 0 → 回退轮询」。
 */

/** 各策略的默认权重与探索率。 */
export const STRATEGY_DEFAULTS = {
  round_robin: { weights: { uptime: 0, latency: 0, throughput: 0 }, exploration_rate: 0 },
  last_used: { weights: { uptime: 0, latency: 0, throughput: 0 }, exploration_rate: 0 },
  latency: { weights: { uptime: 0.1, latency: 0.9, throughput: 0 }, exploration_rate: 0 },
  throughput: { weights: { uptime: 0.1, latency: 0, throughput: 0.9 }, exploration_rate: 0 },
  auto: { weights: { uptime: 0.5, latency: 0.25, throughput: 0.25 }, exploration_rate: 0.01 },
}

/** 阈值与兜底值的默认值。 */
export const THRESHOLD_DEFAULTS = {
  uptime_penalty: 95,
  default_uptime: 100,
  default_latency_ms: 1000,
  default_tokens_per_sec: 30,
}

/** 只有打分策略用权重；其余策略的权重不参与选号。 */
export const SCORING_STRATEGIES = new Set(['latency', 'throughput', 'auto'])

/** 某策略的生效表单值（字符串，便于直接绑定 `input`）。 */
export function fieldsForStrategy(strategy, overrides) {
  const base = STRATEGY_DEFAULTS[strategy] ?? STRATEGY_DEFAULTS.round_robin
  const weights = overrides?.weights ?? {}
  const thresholds = overrides?.thresholds ?? {}
  return {
    weights: {
      uptime: String(weights.uptime ?? base.weights.uptime),
      latency: String(weights.latency ?? base.weights.latency),
      throughput: String(weights.throughput ?? base.weights.throughput),
    },
    thresholds: {
      exploration_rate: String(thresholds.exploration_rate ?? base.exploration_rate),
      uptime_penalty: String(thresholds.uptime_penalty ?? THRESHOLD_DEFAULTS.uptime_penalty),
      default_uptime: String(thresholds.default_uptime ?? THRESHOLD_DEFAULTS.default_uptime),
      default_latency_ms: String(
        thresholds.default_latency_ms ?? THRESHOLD_DEFAULTS.default_latency_ms,
      ),
      default_tokens_per_sec: String(
        thresholds.default_tokens_per_sec ?? THRESHOLD_DEFAULTS.default_tokens_per_sec,
      ),
    },
  }
}
