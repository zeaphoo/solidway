import { useTranslation } from 'react-i18next'
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { formatTokens } from '@/lib/usage'

function chartStyle() {
  return {
    background: 'var(--popover)',
    border: '1px solid var(--border)',
    borderRadius: 'var(--radius)',
    fontSize: 12,
  }
}

export default function UsageTrendCharts({ series, isPending, compact = false, bare = false }) {
  const { t } = useTranslation()
  const data = (series ?? []).map((point) => ({
    ...point,
    label: new Date(point.ts * 1000).toLocaleString(),
  }))

  if (isPending) {
    return <Skeleton className={compact ? 'h-40 w-full' : 'h-56 w-full'} />
  }
  if (!data.length) {
    return (
      <p className={compact ? 'py-10 text-center text-sm text-muted-foreground' : 'py-16 text-center text-sm text-muted-foreground'}>
        {t('usage.empty')}
      </p>
    )
  }

  const Panel = bare ? 'div' : Card
  const PanelHeader = bare ? 'div' : CardHeader
  const PanelBody = bare ? 'div' : CardContent

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Panel className={bare ? 'space-y-2' : undefined}>
        <PanelHeader>
          <CardTitle>{t('usage.requestTrend')}</CardTitle>
        </PanelHeader>
        <PanelBody>
          <div className={compact ? 'h-40 w-full' : 'h-56 w-full'}>
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={data} margin={{ top: 4, right: 8, bottom: 0, left: -16 }}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                <XAxis dataKey="label" hide />
                <YAxis allowDecimals={false} width={40} className="text-xs" />
                <Tooltip contentStyle={chartStyle()} />
                <Area
                  type="monotone"
                  dataKey="requests"
                  name={t('usage.requests')}
                  stroke="var(--chart-2)"
                  fill="var(--chart-2)"
                  fillOpacity={0.2}
                />
                <Area
                  type="monotone"
                  dataKey="errors"
                  name={t('usage.errors')}
                  stroke="var(--destructive)"
                  fill="var(--destructive)"
                  fillOpacity={0.2}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </PanelBody>
      </Panel>

      <Panel className={bare ? 'space-y-2' : undefined}>
        <PanelHeader>
          <CardTitle>{t('usage.tokenTrend')}</CardTitle>
        </PanelHeader>
        <PanelBody>
          <div className={compact ? 'h-40 w-full' : 'h-56 w-full'}>
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={data} margin={{ top: 4, right: 8, bottom: 0, left: -16 }}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                <XAxis dataKey="label" hide />
                <YAxis width={48} tickFormatter={formatTokens} className="text-xs" />
                <Tooltip contentStyle={chartStyle()} formatter={(value) => formatTokens(value)} />
                <Area
                  type="monotone"
                  dataKey="input_tokens"
                  name={t('usage.inputTokens')}
                  stackId="tokens"
                  stroke="var(--chart-2)"
                  fill="var(--chart-2)"
                  fillOpacity={0.25}
                />
                <Area
                  type="monotone"
                  dataKey="output_tokens"
                  name={t('usage.outputTokens')}
                  stackId="tokens"
                  stroke="var(--chart-4)"
                  fill="var(--chart-4)"
                  fillOpacity={0.25}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        </PanelBody>
      </Panel>
    </div>
  )
}
