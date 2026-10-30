import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import UsageBreakdownTable from '@/components/usage/UsageBreakdownTable'
import UsageKpiGrid from '@/components/usage/UsageKpiGrid'
import UsageQueryError from '@/components/usage/UsageQueryError'
import UsageRangeControl from '@/components/usage/UsageRangeControl'
import UsageTrendCharts from '@/components/usage/UsageTrendChartsLazy'
import { SectionCard } from '@/components/ui/section-card'
import { Skeleton } from '@/components/ui/skeleton'
import {
  useUsageBreakdown,
  useUsageConfig,
  useUsageSummary,
  useUsageTimeseries,
} from '@/hooks/useUsage'
import { buildUsageParams, dateInputToEpoch } from '@/lib/usage'

export default function RouteUsageSection({ routeId }) {
  const { t } = useTranslation()
  const [range, setRange] = useState('24h')
  const [fromDate, setFromDate] = useState('')
  const [toDate, setToDate] = useState('')
  const { data: usageConfig } = useUsageConfig()
  const tzOffset = usageConfig?.timezone_offset_minutes
  const customReady = range !== 'custom' || (fromDate && toDate)

  const params = useMemo(
    () => ({
      ...buildUsageParams({
        range,
        from: dateInputToEpoch(fromDate, tzOffset),
        to: dateInputToEpoch(toDate, tzOffset, true),
        route_id: routeId,
      }),
      enabled: Boolean(routeId && customReady),
    }),
    [routeId, range, fromDate, toDate, customReady, tzOffset],
  )

  const summary = useUsageSummary(params)
  const series = useUsageTimeseries(params)
  const models = useUsageBreakdown({ ...params, by: 'model' })
  const providers = useUsageBreakdown({ ...params, by: 'provider' })

  return (
    <SectionCard
      title={t('usage.routeTitle')}
      description={t('usage.routeSubtitle')}
      action={
        <UsageRangeControl
          range={range}
          onRangeChange={setRange}
          from={fromDate}
          to={toDate}
          onFromChange={setFromDate}
          onToChange={setToDate}
          compact
        />
      }
    >
      <div className="space-y-4">
        {summary.isError ? (
          <UsageQueryError error={summary.error} />
        ) : summary.isPending ? (
          <Skeleton className="h-20 w-full" />
        ) : (
          <UsageKpiGrid summary={summary.data} compact />
        )}

        {series.isError ? (
          <UsageQueryError error={series.error} />
        ) : (
          <UsageTrendCharts
            series={series.data}
            isPending={series.isPending}
            compact
            bare
          />
        )}

        <div className="space-y-4 border-t pt-4">
          <div className="space-y-2">
            <div className="text-sm font-medium">{t('usage.modelBreakdown')}</div>
            {models.isError ? (
              <UsageQueryError error={models.error} />
            ) : (
              <UsageBreakdownTable rows={models.data} by="model" isPending={models.isPending} />
            )}
          </div>
          <div className="space-y-2">
            <div className="text-sm font-medium">{t('usage.providerBreakdown')}</div>
            {providers.isError ? (
              <UsageQueryError error={providers.error} />
            ) : (
              <UsageBreakdownTable
                rows={providers.data}
                by="provider"
                isPending={providers.isPending}
              />
            )}
          </div>
        </div>
      </div>
    </SectionCard>
  )
}
