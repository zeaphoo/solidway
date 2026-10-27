import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  useAdminIncidents,
  useCloseIncident,
} from '@/hooks/useIncidents'
import { useClearProviderHealth } from '@/hooks/useProviderHealth'
import { errorMessage, formatUnixCompact } from '@/lib/format'

const EMPTY_ROWS = []

const RANGES = [
  { value: 'all', seconds: null },
  { value: '24h', seconds: 24 * 3600 },
  { value: '7d', seconds: 7 * 24 * 3600 },
  { value: '30d', seconds: 30 * 24 * 3600 },
]

function formatDuration(seconds) {
  if (seconds === null || seconds === undefined) return '-'
  const value = Math.max(0, Math.floor(seconds))
  if (value < 60) return `${value}s`
  if (value < 3600) return `${Math.floor(value / 60)}m`
  if (value < 86400) return `${(value / 3600).toFixed(1)}h`
  return `${(value / 86400).toFixed(1)}d`
}

export default function ProviderIncidentsPage() {
  const { t } = useTranslation()
  const [state, setState] = useState('all')
  const [routeId, setRouteId] = useState('')
  const [providerId, setProviderId] = useState('')
  const [range, setRange] = useState('all')

  const clearHealth = useClearProviderHealth()
  const closeIncident = useCloseIncident()
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))

  useEffect(() => {
    const timer = window.setInterval(() => {
      setNow(Math.floor(Date.now() / 1000))
    }, 30_000)
    return () => window.clearInterval(timer)
  }, [])

  const since = useMemo(() => {
    const item = RANGES.find((entry) => entry.value === range)
    return item?.seconds ? now - item.seconds : undefined
  }, [range, now])

  const params = useMemo(
    () => ({
      state: state === 'all' ? undefined : state,
      route_id: routeId || undefined,
      provider_id: providerId || undefined,
      since,
      limit: 200,
    }),
    [state, routeId, providerId, since],
  )

  const query = useAdminIncidents(params)
  const optionsQuery = useAdminIncidents({ limit: 200 })
  const rows = query.data ?? EMPTY_ROWS
  const optionRows = optionsQuery.data ?? EMPTY_ROWS

  const routeOptions = useMemo(() => {
    const map = new Map()
    for (const row of optionRows) {
      if (!row.route_id) continue
      map.set(row.route_id, row.route_name || row.route_id)
    }
    return [...map.entries()]
      .map(([id, label]) => ({ id, label }))
      .sort((a, b) => a.label.localeCompare(b.label))
  }, [optionRows])

  const providerOptions = useMemo(() => {
    const map = new Map()
    for (const row of optionRows) {
      if (!row.provider_id) continue
      map.set(row.provider_id, row.provider_name || row.provider_id)
    }
    return [...map.entries()]
      .map(([id, label]) => ({ id, label }))
      .sort((a, b) => a.label.localeCompare(b.label))
  }, [optionRows])

  const resetFilters = () => {
    setState('all')
    setRouteId('')
    setProviderId('')
    setRange('all')
  }

  const action = (row) => {
    if (row.state !== 'open') return null
    const pending = clearHealth.isPending || closeIncident.isPending
    const onError = (error) => toast.error(errorMessage(t, error))
    const onSuccess = () => toast.success(t('incidents.cleared'))
    if (row.provider_deleted) {
      return (
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={pending}
          onClick={() => closeIncident.mutate(row.id, { onSuccess, onError })}
        >
          {t('incidents.closeRecord')}
        </Button>
      )
    }
    return (
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={pending}
        onClick={() => clearHealth.mutate(row.provider_id, { onSuccess, onError })}
      >
        {t('incidents.clear')}
      </Button>
    )
  }


  return (
    <div className="mx-auto max-w-6xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">{t('incidents.pageTitle')}</h1>
        <p className="text-sm text-muted-foreground">{t('incidents.pageHint')}</p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Select value={state} onValueChange={setState}>
          <SelectTrigger className="w-32" aria-label={t('incidents.state')}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t('incidents.allStates')}</SelectItem>
            <SelectItem value="open">{t('incidents.stateOpen')}</SelectItem>
            <SelectItem value="closed">{t('incidents.stateClosed')}</SelectItem>
          </SelectContent>
        </Select>

        <Select value={routeId || '__all__'} onValueChange={(value) => setRouteId(value === '__all__' ? '' : value)}>
          <SelectTrigger className="w-48" aria-label={t('incidents.route')}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">{t('incidents.allRoutes')}</SelectItem>
            {routeId && !routeOptions.some((item) => item.id === routeId) ? (
              <SelectItem value={routeId}>{routeId}</SelectItem>
            ) : null}
            {routeOptions.map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={providerId || '__all__'}
          onValueChange={(value) => setProviderId(value === '__all__' ? '' : value)}
        >
          <SelectTrigger className="w-48" aria-label={t('incidents.provider')}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">{t('incidents.allProviders')}</SelectItem>
            {providerId && !providerOptions.some((item) => item.id === providerId) ? (
              <SelectItem value={providerId}>{providerId}</SelectItem>
            ) : null}
            {providerOptions.map((item) => (
              <SelectItem key={item.id} value={item.id}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select value={range} onValueChange={setRange}>
          <SelectTrigger className="w-36" aria-label={t('incidents.range')}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {RANGES.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {t(`incidents.ranges.${item.value}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Button type="button" variant="ghost" size="sm" onClick={resetFilters}>
          {t('incidents.resetFilters')}
        </Button>

        <span className="ml-auto text-sm text-muted-foreground">
          {t('incidents.count', { count: rows.length })}
        </span>
      </div>

      {query.isPending ? (
        <div className="space-y-2">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
        </div>
      ) : query.isError ? (
        <p className="py-8 text-center text-sm text-destructive">
          {errorMessage(t, query.error)}
        </p>
      ) : !rows.length ? (
        <p className="py-16 text-center text-sm text-muted-foreground">
          {t('incidents.emptyHistory')}
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('incidents.state')}</TableHead>
              <TableHead>{t('incidents.provider')}</TableHead>
              <TableHead>{t('incidents.route')}</TableHead>
              <TableHead>{t('incidents.owner')}</TableHead>
              <TableHead>{t('incidents.kind')}</TableHead>
              <TableHead>{t('incidents.severity')}</TableHead>
              <TableHead>{t('incidents.startedAt')}</TableHead>
              <TableHead>{t('incidents.endedAt')}</TableHead>
              <TableHead className="text-right">{t('incidents.duration')}</TableHead>
              <TableHead className="text-right">{t('incidents.attempts')}</TableHead>
              <TableHead>{t('incidents.resolvedBy')}</TableHead>
              <TableHead className="text-right">{t('common.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => {
              const ended = row.ended_at ?? (row.state === 'open' ? now : null)
              const duration = ended === null ? null : ended - row.started_at
              return (
                <TableRow key={row.id}>
                  <TableCell>
                    <Badge variant={row.state === 'open' ? 'destructive' : 'outline'}>
                      {row.state === 'open' ? t('incidents.stateOpen') : t('incidents.stateClosed')}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    {row.provider_name || row.provider || row.provider_id}
                    {row.provider_deleted ? (
                      <span className="ml-1 text-xs text-muted-foreground">
                        {t('common.deletedSuffix')}
                      </span>
                    ) : null}
                  </TableCell>
                  <TableCell>
                    {row.route_name || row.route_id}
                    {row.route_archived ? (
                      <span className="ml-1 text-xs text-muted-foreground">
                        {t('common.archivedSuffix')}
                      </span>
                    ) : null}
                    {row.route_deleted ? (
                      <span className="ml-1 text-xs text-muted-foreground">
                        {t('common.deletedSuffix')}
                      </span>
                    ) : null}
                  </TableCell>
                  <TableCell className="text-muted-foreground">{row.owner_name || '-'}</TableCell>
                  <TableCell className="font-mono text-xs">{row.kind}</TableCell>
                  <TableCell>
                    <span
                      className={
                        row.severity === 'manual_action'
                          ? 'text-destructive'
                          : row.severity === 'degraded'
                            ? 'text-amber-600 dark:text-amber-500'
                            : 'text-muted-foreground'
                      }
                    >
                      {t(`incidents.severities.${row.severity}`)}
                    </span>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {formatUnixCompact(row.started_at)}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {row.ended_at ? formatUnixCompact(row.ended_at) : '-'}
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{formatDuration(duration)}</TableCell>
                  <TableCell className="text-right tabular-nums">{row.attempts}</TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {row.resolved_by || '-'}
                  </TableCell>
                  <TableCell className="text-right whitespace-nowrap">{action(row)}</TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      )}
    </div>
  )
}
