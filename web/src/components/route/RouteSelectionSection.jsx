import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SectionCard } from '@/components/ui/section-card'
import { Skeleton } from '@/components/ui/skeleton'
import { useProviderKeys } from '@/hooks/useProviderKeys'
import { useClearRouteAffinity, useRouteAffinity } from '@/hooks/useRouteAffinity'
import { useSelectionStrategy, useUpdateSelectionStrategy } from '@/hooks/useSelectionStrategy'
import { errorMessage, formatUnixCompact } from '@/lib/format'
import { SCORING_STRATEGIES, fieldsForStrategy } from '@/lib/selectionDefaults'

const STRATEGIES = ['round_robin', 'last_used', 'latency', 'throughput', 'auto']

/** 只有综合加权允许改权重与探索率；其余策略使用内置固定权重。 */
const EDITABLE = 'auto'

/** 会话记录只被 `last_used` 读取。 */
const USES_AFFINITY = 'last_used'

/** 数据面的健康状态 → 既有 i18n 键。 */
const HEALTH_LABEL = {
  healthy: 'health.healthy',
  quarantined: 'health.cooldown',
  retired: 'health.retired',
}

function formatNumber(value, digits = 0) {
  if (value === null || value === undefined) return '—'
  return Number(value).toFixed(digits)
}

/** 指标行 → 与 Provider 列表一致的显示名；无匹配回退 `provider_id` 掩码前缀。 */
function providerLabel(row, providerById) {
  if (row.name) return row.name
  const key = providerById.get(row.provider_id)?.provider
  return key || row.provider_id.slice(0, 8)
}

/** 会话记录行 → Provider 显示名。 */
function providerName(providerId, providerById) {
  const provider = providerById.get(providerId)
  if (!provider) return providerId.slice(0, 8)
  return provider.name || provider.provider || providerId.slice(0, 8)
}

/** 指标新鲜度：与健康状态正交，必须分列展示。 */
function metricStateKey(row) {
  if (row.samples === 0) return 'selection.stateMissing'
  return row.stale ? 'selection.stateStale' : 'selection.stateFresh'
}

export default function RouteSelectionSection({ routeId }) {
  const { t } = useTranslation()
  const query = useSelectionStrategy(routeId)
  const update = useUpdateSelectionStrategy(routeId)
  const { data: providerKeys } = useProviderKeys(routeId)
  const affinityQuery = useRouteAffinity(routeId)
  const clearAffinity = useClearRouteAffinity(routeId)
  const providerById = useMemo(
    () => new Map((providerKeys ?? []).map((provider) => [provider.id, provider])),
    [providerKeys],
  )
  const [strategy, setStrategy] = useState(null)
  const [weights, setWeights] = useState({ uptime: '', latency: '', throughput: '' })
  const [thresholds, setThresholds] = useState({
    exploration_rate: '',
    uptime_penalty: '',
    default_uptime: '',
    default_latency_ms: '',
    default_tokens_per_sec: '',
  })

  const data = query.data
  // 编辑保护：用户动过表单后，10s 指标轮询不得用服务端快照覆盖输入。
  const dirtyRef = useRef(false)
  const dataRouteRef = useRef(routeId)

  useEffect(() => {
    if (!data) return
    // 切换路由时视为一次全新初始化。
    if (dataRouteRef.current !== routeId) {
      dataRouteRef.current = routeId
      dirtyRef.current = false
    }
    if (dirtyRef.current) return
    setStrategy(data.strategy)
    const fields = fieldsForStrategy(data.strategy, data.config)
    setWeights(fields.weights)
    setThresholds(fields.thresholds)
  }, [data, routeId])

  const submit = (event) => {
    event.preventDefault()
    const body = { strategy, config: {} }
    if (strategy === EDITABLE) {
      const number = (value) => (value === '' ? undefined : Number(value))
      const config = {
        weights: {
          uptime: number(weights.uptime),
          latency: number(weights.latency),
          throughput: number(weights.throughput),
        },
        thresholds: {
          exploration_rate: number(thresholds.exploration_rate),
          uptime_penalty: number(thresholds.uptime_penalty),
          default_uptime: number(thresholds.default_uptime),
          default_latency_ms: number(thresholds.default_latency_ms),
          default_tokens_per_sec: number(thresholds.default_tokens_per_sec),
        },
      }
      // 未填写的项不发送（保留默认值）。
      for (const group of ['weights', 'thresholds']) {
        for (const [key, value] of Object.entries(config[group])) {
          if (value === undefined) delete config[group][key]
        }
        if (!Object.keys(config[group]).length) delete config[group]
      }
      body.config = config
    }
    update.mutate(body, {
      onSuccess: () => {
        // 保存成功后允许下一次轮询回填服务端 resolved 值。
        dirtyRef.current = false
        toast.success(t('selection.saved'))
      },
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  const isEditable = strategy === EDITABLE
  const isScoring = SCORING_STRATEGIES.has(strategy)
  const resolved = data?.resolved
  // 只读展示的权重按**当前选中**策略推导（保存前也不会显示上一个策略的值）。
  const derived = useMemo(
    () => fieldsForStrategy(strategy, data?.config),
    [strategy, data?.config],
  )
  const affinityRows = affinityQuery.data ?? []

  const clearOne = (row) => {
    clearAffinity.mutate(
      { model: row.model, session: row.session },
      {
        onSuccess: () => toast.success(t('selection.affinity.cleared')),
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  const clearAll = () => {
    clearAffinity.mutate(
      {},
      {
        onSuccess: () => toast.success(t('selection.affinity.cleared')),
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <SectionCard title={t('selection.title')} description={t('selection.hint')}>
      {query.isPending || !strategy ? (
        <Skeleton className="h-40 w-full" />
      ) : query.isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, query.error)}</p>
      ) : (
        <form onSubmit={submit} className="space-y-5">
          <div className="space-y-2">
            <Label>{t('selection.strategy')}</Label>
            <div className="flex flex-wrap gap-2">
              {STRATEGIES.map((item) => (
                <Button
                  key={item}
                  type="button"
                  variant={strategy === item ? 'secondary' : 'outline'}
                  size="sm"
                  onClick={() => {
                    if (item === strategy) return
                    dirtyRef.current = true
                    setStrategy(item)
                    // 切策略时按新策略的默认值（叠加已保存的覆盖项）重算表单，
                    // 不能沿用上一个策略的 resolved（非打分策略的权重是 0）。
                    const fields = fieldsForStrategy(item, data?.config)
                    setWeights(fields.weights)
                    setThresholds(fields.thresholds)
                  }}
                >
                  {t(`selection.strategies.${item}`)}
                </Button>
              ))}
            </div>
            <p className="text-xs text-muted-foreground">{t(`selection.strategyHints.${strategy}`)}</p>
          </div>

          {isEditable ? (
            <div className="space-y-4 rounded-lg border p-3">
              <div className="grid gap-4 sm:grid-cols-3">
                <div className="space-y-2">
                  <Label htmlFor="selection-uptime-weight">{t('selection.weights.uptime')}</Label>
                  <Input
                    id="selection-uptime-weight"
                    type="number"
                    min="0"
                    max="1"
                    step="0.05"
                    value={weights.uptime}
                    onChange={(e) => {
                      dirtyRef.current = true
                      setWeights((prev) => ({ ...prev, uptime: e.target.value }))
                    }}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="selection-latency-weight">{t('selection.weights.latency')}</Label>
                  <Input
                    id="selection-latency-weight"
                    type="number"
                    min="0"
                    max="1"
                    step="0.05"
                    value={weights.latency}
                    onChange={(e) => {
                      dirtyRef.current = true
                      setWeights((prev) => ({ ...prev, latency: e.target.value }))
                    }}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="selection-throughput-weight">
                    {t('selection.weights.throughput')}
                  </Label>
                  <Input
                    id="selection-throughput-weight"
                    type="number"
                    min="0"
                    max="1"
                    step="0.05"
                    value={weights.throughput}
                    onChange={(e) => {
                      dirtyRef.current = true
                      setWeights((prev) => ({ ...prev, throughput: e.target.value }))
                    }}
                  />
                </div>
              </div>
              <div className="space-y-2">
                <Label htmlFor="selection-exploration">{t('selection.explorationRate')}</Label>
                <Input
                  id="selection-exploration"
                  type="number"
                  min="0"
                  max="1"
                  step="0.01"
                  value={thresholds.exploration_rate}
                  onChange={(e) => {
                    dirtyRef.current = true
                    setThresholds((prev) => ({ ...prev, exploration_rate: e.target.value }))
                  }}
                />
                <p className="text-xs text-muted-foreground">{t('selection.explorationHint')}</p>
              </div>

              <details className="space-y-3">
                <summary className="cursor-pointer text-xs text-muted-foreground">
                  {t('selection.advanced')}
                </summary>
                <div className="mt-3 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
                  <div className="space-y-2">
                    <Label htmlFor="selection-uptime-penalty">{t('selection.uptimePenalty')}</Label>
                    <Input
                      id="selection-uptime-penalty"
                      type="number"
                      min="1"
                      max="100"
                      value={thresholds.uptime_penalty}
                      onChange={(e) => {
                        dirtyRef.current = true
                        setThresholds((prev) => ({ ...prev, uptime_penalty: e.target.value }))
                      }}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="selection-default-uptime">{t('selection.defaultUptime')}</Label>
                    <Input
                      id="selection-default-uptime"
                      type="number"
                      min="1"
                      max="100"
                      value={thresholds.default_uptime}
                      onChange={(e) => {
                        dirtyRef.current = true
                        setThresholds((prev) => ({ ...prev, default_uptime: e.target.value }))
                      }}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="selection-default-latency">
                      {t('selection.defaultLatency')}
                    </Label>
                    <Input
                      id="selection-default-latency"
                      type="number"
                      min="1"
                      max="600000"
                      value={thresholds.default_latency_ms}
                      onChange={(e) => {
                        dirtyRef.current = true
                        setThresholds((prev) => ({ ...prev, default_latency_ms: e.target.value }))
                      }}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="selection-default-tokens">
                      {t('selection.defaultTokensPerSec')}
                    </Label>
                    <Input
                      id="selection-default-tokens"
                      type="number"
                      min="1"
                      step="0.5"
                      value={thresholds.default_tokens_per_sec}
                      onChange={(e) => {
                        dirtyRef.current = true
                        setThresholds((prev) => ({ ...prev, default_tokens_per_sec: e.target.value }))
                      }}
                    />
                  </div>
                </div>
              </details>
            </div>
          ) : isScoring ? (
            <p className="text-xs text-muted-foreground">
              {t('selection.fixedWeights', {
                uptime: formatNumber(Number(derived.weights.uptime), 2),
                latency: formatNumber(Number(derived.weights.latency), 2),
                throughput: formatNumber(Number(derived.weights.throughput), 2),
              })}
            </p>
          ) : null}

          {resolved?.fallback ? (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
              {t('selection.fallbackHint')}
            </p>
          ) : null}

          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>

          <div className="space-y-2 border-t pt-4">
            <div className="text-xs text-muted-foreground">{t('selection.metrics')}</div>
            {data.providers?.length ? (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-xs text-muted-foreground">
                      <th className="py-1 text-left font-normal">{t('providers.provider')}</th>
                      <th className="py-1 text-right font-normal">{t('providers.priority')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.ttfb')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.uptime')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.tokensPerSec')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.samples')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.score')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.metricState')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.health')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {data.providers.map((row) => (
                      <tr key={row.provider_id} className="border-t">
                        <td className="py-1.5">
                          {providerLabel(row, providerById)}
                          {row.enabled ? null : (
                            <span className="ml-2 text-xs text-muted-foreground">
                              {t('selection.disabled')}
                            </span>
                          )}
                        </td>
                        <td className="py-1.5 text-right tabular-nums">{row.priority}</td>
                        <td className="py-1.5 text-right tabular-nums">
                          {formatNumber(row.ttfb_ms)}
                        </td>
                        <td className="py-1.5 text-right tabular-nums">
                          {formatNumber(row.uptime_pct, 1)}
                        </td>
                        <td className="py-1.5 text-right tabular-nums">
                          {formatNumber(row.tokens_per_sec, 1)}
                        </td>
                        <td className="py-1.5 text-right tabular-nums">{row.samples}</td>
                        <td className="py-1.5 text-right tabular-nums">
                          {formatNumber(row.score, 3)}
                        </td>
                        <td className="py-1.5 text-right text-xs text-muted-foreground">
                          {t(metricStateKey(row))}
                        </td>
                        <td className="py-1.5 text-right text-xs text-muted-foreground">
                          {HEALTH_LABEL[row.health] ? t(HEALTH_LABEL[row.health]) : row.health}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">{t('selection.noProviders')}</p>
            )}
          </div>

          <div className="space-y-2 border-t pt-4">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="space-y-1">
                <div className="text-xs text-muted-foreground">
                  {t('selection.affinity.title')} ·{' '}
                  {t('selection.affinity.count', { count: affinityRows.length })}
                </div>
                <p className="text-xs text-muted-foreground">
                  {t('selection.affinity.hint')}
                  {strategy === USES_AFFINITY ? null : ` ${t('selection.affinity.diagnosticOnly')}`}
                </p>
              </div>
              {affinityRows.length ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={clearAffinity.isPending}
                  onClick={clearAll}
                >
                  {t('selection.affinity.clearAll')}
                </Button>
              ) : null}
            </div>
            {affinityRows.length ? (
              <div className="overflow-x-auto">
                <table className="w-full text-sm">
                  <thead>
                    <tr className="text-xs text-muted-foreground">
                      <th className="py-1 text-left font-normal">{t('selection.affinity.model')}</th>
                      <th className="py-1 text-left font-normal">
                        {t('selection.affinity.session')}
                      </th>
                      <th className="py-1 text-left font-normal">
                        {t('selection.affinity.provider')}
                      </th>
                      <th className="py-1 text-right font-normal">
                        {t('selection.affinity.expiresAt')}
                      </th>
                      <th className="py-1 text-right font-normal" />
                    </tr>
                  </thead>
                  <tbody>
                    {affinityRows.map((row) => (
                      <tr key={`${row.model}|${row.session}`} className="border-t">
                        <td className="py-1.5">{row.model}</td>
                        <td className="py-1.5 font-mono text-xs">{row.session}</td>
                        <td className="py-1.5">{providerName(row.provider_id, providerById)}</td>
                        <td className="py-1.5 text-right tabular-nums text-xs">
                          {formatUnixCompact(row.expires_at)}
                        </td>
                        <td className="py-1.5 text-right">
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            disabled={clearAffinity.isPending}
                            onClick={() => clearOne(row)}
                          >
                            {t('selection.affinity.clearOne')}
                          </Button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">{t('selection.affinity.empty')}</p>
            )}
          </div>
        </form>
      )}
    </SectionCard>
  )
}
