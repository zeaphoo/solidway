import { useTranslation } from 'react-i18next'

import { formatMs, formatPercent, formatTokens, totalTokens } from '@/lib/usage'

function Kpi({ label, value, hint }) {
  return (
    <div className="rounded-lg border p-4">
      <p className="text-xs text-muted-foreground">{label}</p>
      <p className="mt-1 text-xl font-semibold tabular-nums">{value}</p>
      {hint ? <p className="mt-1 text-[11px] text-muted-foreground">{hint}</p> : null}
    </div>
  )
}

export default function UsageKpiGrid({ summary, compact = false }) {
  const { t } = useTranslation()
  const data = summary ?? {}

  return (
    <div
      className={
        compact
          ? 'grid grid-cols-2 gap-3 lg:grid-cols-4'
          : 'grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4'
      }
    >
      <Kpi label={t('usage.requests')} value={data.requests ?? 0} />
      <Kpi label={t('usage.errorRate')} value={formatPercent(data.error_rate)} />
      <Kpi
        label={t('usage.totalTokens')}
        value={formatTokens(totalTokens(data))}
        hint={`${formatTokens(data.input_tokens)} / ${formatTokens(data.output_tokens)}`}
      />
      <Kpi label={t('usage.p95')} value={formatMs(data.p95_ms)} />
      {!compact ? (
        <>
          <Kpi label={t('usage.inputTokens')} value={formatTokens(data.input_tokens)} />
          <Kpi label={t('usage.outputTokens')} value={formatTokens(data.output_tokens)} />
          <Kpi label={t('usage.cachedTokens')} value={formatTokens(data.cached_tokens)} />
          <Kpi label={t('usage.failovers')} value={data.failovers ?? 0} />
        </>
      ) : null}
    </div>
  )
}
