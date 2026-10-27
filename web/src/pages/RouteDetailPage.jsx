import { ArchiveIcon, PencilIcon, PlusIcon } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate, useOutletContext, useParams, useSearchParams } from 'react-router'
import { toast } from 'sonner'

import ConfirmDialog from '@/components/ConfirmDialog'
import ProviderKeyFormDialog from '@/components/route/ProviderKeyFormDialog'
import RouteAccessSection from '@/components/route/RouteAccessSection'
import IncidentBanner from '@/components/route/IncidentBanner'
import RouteSelectionSection from '@/components/route/RouteSelectionSection'
import RouteUsageSection from '@/components/route/RouteUsageSection'
import ProviderKeyTable from '@/components/route/ProviderKeyTable'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  useDeleteProviderKey,
  useProviderKeys,
  useUpdateProviderKey,
} from '@/hooks/useProviderKeys'
import { useArchiveRoute, useRoute, useUpdateRoute } from '@/hooks/useRoutes'
import { useSession } from '@/hooks/useAuth'
import { useClearProviderHealth, useProviderHealth } from '@/hooks/useProviderHealth'
import { formatsAndModels } from '@/lib/accessSnippets'
import { errorMessage, formatUnix, formatUnixCompact } from '@/lib/format'

/** 页头次要信息：小号单行、以间隔点分隔，避免与正文争夺注意力。 */
function MetaItem({ at, label }) {
  return (
    <span className="whitespace-nowrap">
      {label}{' '}
      <time
        dateTime={new Date(at * 1000).toISOString()}
        title={formatUnix(at)}
        className="tabular-nums"
      >
        {formatUnixCompact(at)}
      </time>
    </span>
  )
}

function RouteMeta({ route }) {
  const { t } = useTranslation()
  const items = []
  if (route.updated_at) {
    items.push({ key: 'updated', at: route.updated_at, label: t('routes.updatedAtValue') })
  }
  if (route.created_at) {
    items.push({ key: 'created', at: route.created_at, label: t('routes.createdAtValue') })
  }
  if (!items.length) return null

  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 pt-1 text-xs text-muted-foreground">
      {items.map((item, index) => (
        <span key={item.key} className="flex items-center gap-2">
          {index > 0 ? <span aria-hidden="true">·</span> : null}
          <MetaItem at={item.at} label={item.label} />
        </span>
      ))}
    </div>
  )
}

export default function RouteDetailPage() {
  const { t } = useTranslation()
  const { id } = useParams()
  const navigate = useNavigate()
  const { openEditRoute } = useOutletContext()

  const routeQuery = useRoute(id)
  // 路由不存在时不加载子资源，避免旧链接在控制台打出一片 404。
  const route = routeQuery.data
  const routeId = route?.id
  const missing = routeQuery.isError && routeQuery.error?.status === 404

  useEffect(() => {
    if (missing) navigate('/routes', { replace: true })
  }, [missing, navigate])

  const keysQuery = useProviderKeys(routeId)
  const deleteKeyMutation = useDeleteProviderKey(routeId)
  const updateKeyMutation = useUpdateProviderKey(routeId)
  const updateRouteMutation = useUpdateRoute()
  const healthQuery = useProviderHealth()
  const clearHealth = useClearProviderHealth()
  const { data: session } = useSession()
  const canRestore = session?.role === 'admin'
  const healthById = Object.fromEntries(
    (healthQuery.data ?? []).map((entry) => [entry.provider_id, entry]),
  )
  const archiveRouteMutation = useArchiveRoute()
  const archived = Boolean(route?.archived_at)

  const { availableFormats, modelsByFormat } = useMemo(
    () => formatsAndModels(keysQuery.data),
    [keysQuery.data],
  )

  const [keyFormOpen, setKeyFormOpen] = useState(false)
  const [editingKey, setEditingKey] = useState(null)
  const [deletingKey, setDeletingKey] = useState(null)
  const [deleteRouteOpen, setDeleteRouteOpen] = useState(false)

  // 快速开始可直达「新增 Provider」：?new=provider 打开对话框后立即清理参数，避免刷新重开。
  const [searchParams, setSearchParams] = useSearchParams()
  useEffect(() => {
    if (searchParams.get('new') !== 'provider') return
    // 路由未就绪时不消费参数，等详情加载完再打开。
    if (!routeId) return
    if (!archived) setKeyFormOpen(true)
    const next = new URLSearchParams(searchParams)
    next.delete('new')
    setSearchParams(next, { replace: true })
  }, [searchParams, setSearchParams, archived, routeId])

  const openCreateKey = () => {
    setEditingKey(null)
    setKeyFormOpen(true)
  }

  const openEditKey = (row) => {
    setEditingKey(row)
    setKeyFormOpen(true)
  }

  const confirmDeleteKey = () => {
    if (!deletingKey) return
    deleteKeyMutation.mutate(deletingKey.id, {
      onSuccess: () => {
        toast.success(t('providers.deleted'))
        setDeletingKey(null)
      },
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  const toggleRoute = (route, enabled) => {
    updateRouteMutation.mutate(
      { id: route.id, body: { enabled } },
      { onError: (err) => toast.error(errorMessage(t, err)) },
    )
  }

  const toggleKeyStatus = (row, checked) => {
    updateKeyMutation.mutate(
      { id: row.id, body: { status: checked ? 'active' : 'inactive' } },
      { onError: (err) => toast.error(errorMessage(t, err)) },
    )
  }

  const restoreHealth = (row) => {
    clearHealth.mutate(row.id, {
      onSuccess: () => toast.success(t('health.restored')),
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  const confirmArchiveRoute = () => {
    archiveRouteMutation.mutate(routeId, {
      onSuccess: () => {
        toast.success(t('routes.archived'))
        setDeleteRouteOpen(false)
      },
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  if (missing) return null
  if (routeQuery.isError) {
    return <p className="text-sm text-destructive">{errorMessage(t, routeQuery.error)}</p>
  }

  return (
    <div className="mx-auto max-w-5xl space-y-8">
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0 space-y-1">
          {routeQuery.isPending ? (
            <>
              <Skeleton className="h-7 w-48" />
              <Skeleton className="h-4 w-72" />
              <Skeleton className="h-3 w-56" />
            </>
          ) : (
            <>
              <div className="flex items-center gap-2">
                <h1 className="truncate text-xl font-semibold">{route.name}</h1>
                {route.archived_at ? (
                  <Badge variant="outline">{t('routes.archivedBadge')}</Badge>
                ) : null}
              </div>
              <p className="text-sm text-muted-foreground">{route.description || null}</p>
              <RouteMeta route={route} />
            </>
          )}
        </div>

        {route ? (
          <div className="flex shrink-0 items-center gap-2">
            {archived ? null : (
              <>
                <Switch
                  checked={Boolean(route.enabled)}
                  disabled={
                    updateRouteMutation.isPending && updateRouteMutation.variables?.id === id
                  }
                  onCheckedChange={(checked) => toggleRoute(route, checked)}
                  aria-label={t('common.enabled')}
                />
                <Button variant="outline" size="sm" onClick={() => openEditRoute(route)}>
                  <PencilIcon className="size-4" />
                  {t('common.edit')}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  className="text-destructive hover:text-destructive"
                  onClick={() => setDeleteRouteOpen(true)}
                >
                  <ArchiveIcon className="size-4" />
                  {t('routes.archive')}
                </Button>
              </>
            )}
          </div>
        ) : null}
      </div>

      {archived ? (
        <div className="rounded-lg border border-amber-500/40 bg-amber-500/10 px-4 py-3 text-sm">
          {t('routes.archivedBanner')}
        </div>
      ) : null}

      <IncidentBanner routeId={routeId} />

      <RouteAccessSection
        routeId={routeId}
        modelsByFormat={modelsByFormat}
        formats={availableFormats}
        readOnly={archived}
      />

      {archived ? null : <RouteSelectionSection routeId={routeId} />}

      <section className="space-y-3">
        <div className="flex items-center justify-between gap-4">
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-base font-semibold">{t('providers.title')}</h2>
              {keysQuery.data ? (
                <Badge variant="secondary" className="tabular-nums">
                  {keysQuery.data.length}
                </Badge>
              ) : null}
            </div>
            <p className="text-sm text-muted-foreground">{t('providers.maskedHint')}</p>
          </div>
          {archived ? null : (
            <Button onClick={openCreateKey}>
              <PlusIcon className="size-4" />
              {t('providers.new')}
            </Button>
          )}
        </div>

        {keysQuery.isPending ? (
          <div className="space-y-2">
            <Skeleton className="h-9 w-full" />
            <Skeleton className="h-9 w-full" />
          </div>
        ) : keysQuery.isError ? (
          <p className="py-8 text-center text-sm text-destructive">
            {errorMessage(t, keysQuery.error)}
          </p>
        ) : (
          <ProviderKeyTable
            rows={keysQuery.data ?? []}
            onEdit={openEditKey}
            onDelete={setDeletingKey}
            onToggleStatus={toggleKeyStatus}
            pendingId={updateKeyMutation.isPending ? updateKeyMutation.variables?.id : undefined}
            healthById={healthById}
            onRestore={restoreHealth}
            canRestore={canRestore && !archived}
            readOnly={archived}
          />
        )}
      </section>

      <RouteUsageSection routeId={routeId} />

      <ProviderKeyFormDialog
        open={keyFormOpen}
        onOpenChange={setKeyFormOpen}
        routeId={routeId}
        providerKey={editingKey}
      />

      <ConfirmDialog
        open={Boolean(deletingKey)}
        onOpenChange={(open) => !open && setDeletingKey(null)}
        title={t('providers.deleteConfirmTitle')}
        description={t('providers.deleteConfirmDesc')}
        onConfirm={confirmDeleteKey}
        pending={deleteKeyMutation.isPending}
      />

      <ConfirmDialog
        open={deleteRouteOpen}
        onOpenChange={setDeleteRouteOpen}
        title={t('routes.archiveConfirmTitle')}
        description={t('routes.archiveConfirmDesc')}
        onConfirm={confirmArchiveRoute}
        pending={archiveRouteMutation.isPending}
      />
    </div>
  )
}
