import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { SectionCard } from '@/components/ui/section-card'
import { Skeleton } from '@/components/ui/skeleton'
import { useProviderKeys } from '@/hooks/useProviderKeys'
import { useSelectionStrategy, useUpdateSelectionStrategy } from '@/hooks/useSelectionStrategy'
import { errorMessage } from '@/lib/format'

const STRATEGIES = ['round_robin', 'uptime', 'latency', 'auto']

/** 只有综合加权允许改权重与探索率；其余策略使用内置固定权重。 */
const EDITABLE = 'auto'

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

export default function RouteSelectionSection({ routeId }) {
  const { t } = useTranslation()
  const query = useSelectionStrategy(routeId)
  const update = useUpdateSelectionStrategy(routeId)
  const { data: providerKeys } = useProviderKeys(routeId)
  const providerById = useMemo(
    () => new Map((providerKeys ?? []).map((provider) => [provider.id, provider])),
    [providerKeys],
  )
  const [strategy, setStrategy] = useState(null)
  const [weights, setWeights] = useState({ uptime: '', latency: '' })
  const [thresholds, setThresholds] = useState({
    exploration_rate: '',
    uptime_penalty: '',
    default_uptime: '',
    default_latency_ms: '',
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
    const resolved = data.resolved ?? {}
    setWeights({
      uptime: String(resolved.weights?.uptime ?? ''),
      latency: String(resolved.weights?.latency ?? ''),
    })
    setThresholds({
      exploration_rate: String(resolved.thresholds?.exploration_rate ?? ''),
      uptime_penalty: String(resolved.thresholds?.uptime_penalty ?? ''),
      default_uptime: String(resolved.thresholds?.default_uptime ?? ''),
      default_latency_ms: String(resolved.thresholds?.default_latency_ms ?? ''),
    })
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
        },
        thresholds: {
          exploration_rate: number(thresholds.exploration_rate),
          uptime_penalty: number(thresholds.uptime_penalty),
          default_uptime: number(thresholds.default_uptime),
          default_latency_ms: number(thresholds.default_latency_ms),
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
  const resolved = data?.resolved

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
                    if (item !== strategy) dirtyRef.current = true
                    setStrategy(item)
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
              <div className="grid gap-4 sm:grid-cols-2">
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
              </div>
              <div className="grid gap-4 sm:grid-cols-2">
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
              </div>

              <details className="space-y-3">
                <summary className="cursor-pointer text-xs text-muted-foreground">
                  {t('selection.advanced')}
                </summary>
                <div className="mt-3 grid gap-4 sm:grid-cols-3">
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
                </div>
              </details>
            </div>
          ) : (
            <p className="text-xs text-muted-foreground">
              {t('selection.fixedWeights', {
                uptime: formatNumber(resolved?.weights?.uptime, 2),
                latency: formatNumber(resolved?.weights?.latency, 2),
              })}
            </p>
          )}

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
                      <th className="py-1 text-right font-normal">{t('selection.samples')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.score')}</th>
                      <th className="py-1 text-right font-normal">{t('selection.metricState')}</th>
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
                        <td className="py-1.5 text-right tabular-nums">{row.samples}</td>
                        <td className="py-1.5 text-right tabular-nums">
                          {formatNumber(row.score, 3)}
                        </td>
                        <td className="py-1.5 text-right text-xs text-muted-foreground">
                          {row.samples === 0
                            ? t('selection.stateMissing')
                            : row.stale
                              ? t('selection.stateStale')
                              : HEALTH_LABEL[row.health]
                                ? t(HEALTH_LABEL[row.health])
                                : row.health}
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
        </form>
      )}
    </SectionCard>
  )
}
