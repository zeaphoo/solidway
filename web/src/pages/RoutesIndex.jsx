import { ArchiveIcon, ArrowRightIcon, PlusIcon, RouteIcon, SearchIcon } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useOutletContext } from 'react-router'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useIncidents } from '@/hooks/useIncidents'
import { useProviderHealth } from '@/hooks/useProviderHealth'
import { useRoutes } from '@/hooks/useRoutes'
import { errorMessage, formatUnix } from '@/lib/format'

/** 按名称或描述过滤（大小写不敏感；空关键字返回全部）。 */
function filterRoutes(routes, query) {
  const keyword = query.trim().toLowerCase()
  if (!keyword) return routes
  return routes.filter(
    (route) =>
      route.name.toLowerCase().includes(keyword) ||
      (route.description ?? '').toLowerCase().includes(keyword),
  )
}

export default function RoutesIndex() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const [showArchived, setShowArchived] = useState(false)
  const { data: routes, isPending, isError, error } = useRoutes({
    includeArchived: showArchived,
  })
  const { data: openIncidents = [] } = useIncidents({ state: 'open', limit: 200 })
  const { data: providerHealth = [] } = useProviderHealth()
  const { openCreateRoute } = useOutletContext()
  const [query, setQuery] = useState('')

  const all = useMemo(() => routes ?? [], [routes])
  const matched = useMemo(() => filterRoutes(all, query), [all, query])
  const incidentsByRoute = useMemo(() => {
    const map = new Map()
    for (const incident of openIncidents) {
      const list = map.get(incident.route_id) ?? []
      list.push(incident)
      map.set(incident.route_id, list)
    }
    return map
  }, [openIncidents])
  const abnormalByRoute = useMemo(() => {
    const map = new Map()
    for (const entry of providerHealth) {
      map.set(entry.route_id, (map.get(entry.route_id) ?? 0) + 1)
    }
    return map
  }, [providerHealth])

  const healthBadge = (route) => {
    if (route.archived_at) {
      return <span className="text-sm text-muted-foreground">—</span>
    }
    const incidents = incidentsByRoute.get(route.id) ?? []
    const manual = incidents.filter((row) => row.severity === 'manual_action').length
    if (manual) {
      return <Badge variant="destructive">{t('routes.healthManual', { count: manual })}</Badge>
    }
    if (incidents.length) {
      return <Badge variant="secondary">{t('routes.healthFaults', { count: incidents.length })}</Badge>
    }
    const abnormal = abnormalByRoute.get(route.id) ?? 0
    if (abnormal) {
      return (
        <Badge variant="outline" className="border-amber-500/50 text-amber-700 dark:text-amber-400">
          {t('routes.healthAbnormal', { count: abnormal })}
        </Badge>
      )
    }
    return <Badge variant="secondary">{t('routes.healthNormal')}</Badge>
  }

  if (isPending) {
    return (
      <div className="mx-auto max-w-5xl space-y-2">
        <Skeleton className="h-9 w-full" />
        <Skeleton className="h-9 w-full" />
      </div>
    )
  }

  if (isError) {
    return (
      <div className="mx-auto max-w-5xl">
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      </div>
    )
  }

  if (!all.length) {
    return (
      <div className="flex min-h-[60vh] flex-col items-center justify-center gap-4 text-center">
        <span className="grid size-12 place-items-center rounded-full bg-muted">
          <RouteIcon className="size-6 text-muted-foreground" />
        </span>
        <div className="space-y-1">
          <h2 className="text-lg font-semibold">{t('routes.title')}</h2>
          <p className="text-sm text-muted-foreground">{t('routes.empty')}</p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant={showArchived ? 'secondary' : 'outline'}
            onClick={() => setShowArchived((value) => !value)}
          >
            <ArchiveIcon className="size-4" />
            {t('routes.showArchived')}
          </Button>
          <Button onClick={openCreateRoute}>
            <PlusIcon className="size-4" />
            {t('routes.new')}
          </Button>
        </div>
        <Link
          to="/welcome"
          className="inline-flex items-center gap-1 text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
        >
          {t('routes.onboardingHint')}
          <ArrowRightIcon className="size-3.5" />
        </Link>
      </div>
    )
  }

  return (
    <div className="mx-auto max-w-5xl space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="space-y-1">
          <h2 className="text-lg font-semibold">{t('routes.title')}</h2>
          <p className="text-sm text-muted-foreground">{t('routes.subtitle')}</p>
        </div>
        <div className="flex items-center gap-2">
          <div className="relative">
            <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t('routes.searchPlaceholder')}
              aria-label={t('routes.searchPlaceholder')}
              className="w-64 pl-8"
            />
          </div>
          <Button
            variant={showArchived ? 'secondary' : 'outline'}
            onClick={() => setShowArchived((value) => !value)}
          >
            <ArchiveIcon className="size-4" />
            {t('routes.showArchived')}
          </Button>
          <Button onClick={openCreateRoute}>
            <PlusIcon className="size-4" />
            {t('routes.new')}
          </Button>
        </div>
      </div>

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t('routes.name')}</TableHead>
            <TableHead>{t('routes.description')}</TableHead>
            <TableHead className="w-24">{t('common.status')}</TableHead>
            <TableHead className="w-32">{t('routes.health')}</TableHead>
            <TableHead className="w-44">{t('routes.updatedAt')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {matched.length ? (
            matched.map((route) => (
              <TableRow
                key={route.id}
                className="cursor-pointer"
                onClick={() => navigate(`/routes/${route.id}`)}
              >
                <TableCell className="font-medium">
                  {route.name}
                  {route.archived_at ? (
                    <span className="ml-1 text-xs text-muted-foreground">
                      #{route.id.slice(-6)}
                    </span>
                  ) : null}
                </TableCell>
                <TableCell className="max-w-[420px] truncate text-muted-foreground">
                  {route.description || '—'}
                </TableCell>
                <TableCell>
                  {route.archived_at ? (
                    <Badge variant="outline">{t('routes.archivedBadge')}</Badge>
                  ) : route.enabled ? (
                    <Badge variant="secondary">{t('common.enabled')}</Badge>
                  ) : (
                    <Badge variant="outline">{t('common.disabled')}</Badge>
                  )}
                </TableCell>
                <TableCell>{healthBadge(route)}</TableCell>
                <TableCell className="text-muted-foreground tabular-nums">
                  {formatUnix(route.updated_at)}
                </TableCell>
              </TableRow>
            ))
          ) : (
            <TableRow>
              <TableCell colSpan={5} className="py-8 text-center text-sm text-muted-foreground">
                {t('routes.searchEmpty')}
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
