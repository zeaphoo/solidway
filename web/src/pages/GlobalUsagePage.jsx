import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import FilterSelect from '@/components/usage/FilterSelect'
import UsageBreakdownTable from '@/components/usage/UsageBreakdownTable'
import UsageKpiGrid from '@/components/usage/UsageKpiGrid'
import UsageQueryError from '@/components/usage/UsageQueryError'
import UsageRangeControl from '@/components/usage/UsageRangeControl'
import UsageRequestTable from '@/components/usage/UsageRequestTable'
import UsageTrendCharts from '@/components/usage/UsageTrendChartsLazy'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import {
  useUsageBreakdown,
  useUsageConfig,
  useUsageRequests,
  useUsageSummary,
  useUsageTimeseries,
} from '@/hooks/useUsage'
import { buildUsageParams, dateInputToEpoch } from '@/lib/usage'

export default function GlobalUsagePage() {
  const { t } = useTranslation()
  const [range, setRange] = useState('24h')
  const [fromDate, setFromDate] = useState('')
  const [toDate, setToDate] = useState('')
  const [by, setBy] = useState('owner')
  const [filters, setFilters] = useState({})
  const { data: usageConfig } = useUsageConfig()
  const tzOffset = usageConfig?.timezone_offset_minutes

  const customReady = range !== 'custom' || (fromDate && toDate)
  const params = useMemo(
    () => ({
      ...buildUsageParams({
        range,
        from: dateInputToEpoch(fromDate, tzOffset),
        to: dateInputToEpoch(toDate, tzOffset, true),
        owner_id: filters.owner_id,
        route_id: filters.route_id,
        api_key_id: filters.api_key_id,
        model: filters.model,
        provider_id: filters.provider_id,
      }),
      scope: 'global',
      enabled: Boolean(customReady),
    }),
    [range, fromDate, toDate, filters, customReady, tzOffset],
  )

  const summary = useUsageSummary(params)
  const series = useUsageTimeseries(params)
  const breakdown = useUsageBreakdown({ ...params, by })
  const requests = useUsageRequests(params)

  const optionBase = {
    ...params,
    owner_id: undefined,
    route_id: undefined,
    api_key_id: undefined,
    model: undefined,
    provider_id: undefined,
    by: undefined,
  }
  const ownerOptions = useUsageBreakdown({ ...optionBase, by: 'owner' })
  const routeOptions = useUsageBreakdown({ ...optionBase, by: 'route' })
  const modelOptions = useUsageBreakdown({ ...optionBase, by: 'model' })
  const providerOptions = useUsageBreakdown({ ...optionBase, by: 'provider' })
  const apiKeyOptions = useUsageBreakdown({ ...optionBase, by: 'api_key' })

  const setFilter = (key) => (value) => setFilters((prev) => ({ ...prev, [key]: value }))

  return (
    <div className="mx-auto max-w-6xl space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">{t('usage.globalTitle')}</h1>
          <p className="text-sm text-muted-foreground">{t('usage.globalSubtitle')}</p>
        </div>
        <UsageRangeControl
          range={range}
          onRangeChange={setRange}
          from={fromDate}
          to={toDate}
          onFromChange={setFromDate}
          onToChange={setToDate}
        />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <FilterSelect
          label={t('usage.allOwners')}
          value={filters.owner_id}
          onChange={setFilter('owner_id')}
          options={(ownerOptions.data ?? []).map((row) => ({ value: row.key, label: row.label }))}
        />
        <FilterSelect
          label={t('usage.allRoutes')}
          value={filters.route_id}
          onChange={setFilter('route_id')}
          options={(routeOptions.data ?? []).map((row) => ({
            value: row.key,
            label: row.owner_name ? `${row.owner_name} / ${row.label}` : row.label,
          }))}
        />
        <FilterSelect
          label={t('usage.allApiKeys')}
          value={filters.api_key_id}
          onChange={setFilter('api_key_id')}
          options={(apiKeyOptions.data ?? []).map((row) => ({
            value: row.key,
            label: row.label,
          }))}
        />
        <FilterSelect
          label={t('usage.allModels')}
          value={filters.model}
          onChange={setFilter('model')}
          options={(modelOptions.data ?? []).map((row) => ({ value: row.key, label: row.label }))}
        />
        <FilterSelect
          label={t('usage.allProviders')}
          value={filters.provider_id}
          onChange={setFilter('provider_id')}
          options={(providerOptions.data ?? []).map((row) => ({
            value: row.key,
            label: row.label,
          }))}
        />
      </div>

      {summary.isError ? (
        <UsageQueryError error={summary.error} />
      ) : summary.isPending ? (
        <Skeleton className="h-24 w-full" />
      ) : (
        <UsageKpiGrid summary={summary.data} />
      )}

      {series.isError ? (
        <UsageQueryError error={series.error} />
      ) : (
        <UsageTrendCharts series={series.data} isPending={series.isPending} />
      )}

      <Card>
        <CardHeader className="flex-row items-center justify-between">
          <CardTitle>{t('usage.breakdown')}</CardTitle>
          <Select value={by} onValueChange={setBy}>
            <SelectTrigger className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="owner">{t('usage.byOwner')}</SelectItem>
              <SelectItem value="route">{t('usage.byRoute')}</SelectItem>
              <SelectItem value="api_key">{t('usage.byApiKey')}</SelectItem>
              <SelectItem value="model">{t('usage.byModel')}</SelectItem>
              <SelectItem value="provider">{t('usage.byProvider')}</SelectItem>
            </SelectContent>
          </Select>
        </CardHeader>
        <CardContent>
          {breakdown.isError ? (
            <UsageQueryError error={breakdown.error} />
          ) : (
            <UsageBreakdownTable
              rows={breakdown.data}
              by={by}
              isPending={breakdown.isPending}
              showOwner
            />
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t('usage.recent')}</CardTitle>
        </CardHeader>
        <CardContent>
          {requests.isError ? (
            <UsageQueryError error={requests.error} />
          ) : requests.isPending ? (
            <Skeleton className="h-32 w-full" />
          ) : !requests.data?.length ? (
            <p className="py-8 text-center text-sm text-muted-foreground">
              {t('usage.recentEmpty')}
            </p>
          ) : (
            <UsageRequestTable rows={requests.data} showOwner />
          )}
        </CardContent>
      </Card>
    </div>
  )
}
