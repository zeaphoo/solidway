import { CalendarClockIcon, PlusIcon, SearchIcon, TrashIcon } from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { toast } from 'sonner'

import ConfirmDialog from '@/components/ConfirmDialog'
import CreateApiKeyDialog from '@/components/api-keys/CreateApiKeyDialog'
import CreatedApiKeyDialog from '@/components/api-keys/CreatedApiKeyDialog'
import ApiKeyExpiryCell from '@/components/api-keys/ApiKeyExpiryCell'
import EditApiKeyExpiryDialog from '@/components/api-keys/EditApiKeyExpiryDialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useApiKeys, useDeleteApiKey, useUpdateApiKey } from '@/hooks/useApiKeys'
import { useRoutes } from '@/hooks/useRoutes'
import { errorMessage, formatUnix } from '@/lib/format'

const PAGE_SIZE = 20

export default function ApiKeysPage() {
  const { t } = useTranslation()
  const [searchParams, setSearchParams] = useSearchParams()
  const { data: keys, isPending, isError, error } = useApiKeys()
  const { data: routes } = useRoutes()
  const updateMutation = useUpdateApiKey()
  const deleteMutation = useDeleteApiKey()

  const query = searchParams.get('q') ?? ''
  const routeFilter = searchParams.get('route') ?? ''
  const routeValue = (routes ?? []).some((route) => route.id === routeFilter)
    ? routeFilter
    : '__all__'
  const effectiveRoute = routeValue === '__all__' ? '' : routeFilter

  const [page, setPage] = useState(1)
  const [createOpen, setCreateOpen] = useState(false)
  const [createdKey, setCreatedKey] = useState(null)
  const [editingExpiry, setEditingExpiry] = useState(null)
  const [deleting, setDeleting] = useState(null)

  // 快速开始可直达创建：?new=1 打开对话框后立即清理参数，避免刷新重开。
  useEffect(() => {
    if (searchParams.get('new') !== '1') return
    setCreateOpen(true)
    const next = new URLSearchParams(searchParams)
    next.delete('new')
    setSearchParams(next, { replace: true })
  }, [searchParams, setSearchParams])

  const updateFilter = (key, value) => {
    const next = new URLSearchParams(searchParams)
    if (value) next.set(key, value)
    else next.delete(key)
    setSearchParams(next, { replace: true })
    setPage(1)
  }

  const filtered = useMemo(() => {
    const keyword = query.trim().toLowerCase()
    return (keys ?? []).filter((row) => {
      const matchName = !keyword || (row.name ?? '').toLowerCase().includes(keyword)
      const matchRoute = !effectiveRoute || row.route_id === effectiveRoute
      return matchName && matchRoute
    })
  }, [keys, query, effectiveRoute])

  const pageCount = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
  const currentPage = Math.min(page, pageCount)
  const paged = filtered.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)

  const toggleStatus = (row, checked) => {
    updateMutation.mutate(
      { id: row.id, body: { status: checked ? 'active' : 'inactive' } },
      { onError: (err) => toast.error(errorMessage(t, err)) },
    )
  }

  const confirmDelete = () => {
    if (!deleting) return
    deleteMutation.mutate(deleting.id, {
      onSuccess: () => {
        toast.success(t('apiKeys.deleted'))
        setDeleting(null)
      },
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">{t('apiKeys.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('apiKeys.subtitle')}</p>
        </div>
        <Button onClick={() => setCreateOpen(true)} disabled={!routes?.length}>
          <PlusIcon className="size-4" />
          {t('apiKeys.new')}
        </Button>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <div className="relative">
          <SearchIcon className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(event) => updateFilter('q', event.target.value)}
            placeholder={t('apiKeys.searchPlaceholder')}
            aria-label={t('apiKeys.searchPlaceholder')}
            className="w-64 pl-8"
          />
        </div>
        <Select
          value={routeValue}
          onValueChange={(value) => updateFilter('route', value === '__all__' ? '' : value)}
        >
          <SelectTrigger className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">{t('apiKeys.allRoutes')}</SelectItem>
            {(routes ?? []).map((route) => (
              <SelectItem key={route.id} value={route.id}>
                {route.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-sm text-muted-foreground">
          {t('apiKeys.filteredCount', { count: filtered.length })}
        </span>
      </div>

      {isPending ? (
        <div className="space-y-2">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
        </div>
      ) : isError ? (
        <p className="py-8 text-center text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : !keys?.length ? (
        <p className="py-16 text-center text-sm text-muted-foreground">{t('apiKeys.empty')}</p>
      ) : !filtered.length ? (
        <p className="py-16 text-center text-sm text-muted-foreground">
          {t('apiKeys.searchEmpty')}
        </p>
      ) : (
        <>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t('apiKeys.route')}</TableHead>
                <TableHead>{t('apiKeys.name')}</TableHead>
                <TableHead>{t('apiKeys.prefix')}</TableHead>
                <TableHead>{t('apiKeys.masked')}</TableHead>
                <TableHead>{t('apiKeys.expiresAt')}</TableHead>
                <TableHead>{t('providers.status')}</TableHead>
                <TableHead>{t('apiKeys.createdAt')}</TableHead>
                <TableHead className="text-right">{t('common.actions')}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {paged.map((row) => (
                <TableRow key={row.id}>
                  <TableCell className="font-medium">{row.route_name || '-'}</TableCell>
                  <TableCell>{row.name || '-'}</TableCell>
                  <TableCell className="font-mono text-xs">{row.key_prefix}</TableCell>
                  <TableCell className="font-mono text-xs">{row.key_masked}</TableCell>
                  <TableCell>
                    <ApiKeyExpiryCell expiresAt={row.expires_at} />
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={row.status === 'active'}
                      disabled={
                        updateMutation.isPending && updateMutation.variables?.id === row.id
                      }
                      onCheckedChange={(checked) => toggleStatus(row, checked)}
                      aria-label={t('providers.status')}
                    />
                  </TableCell>
                  <TableCell className="text-muted-foreground tabular-nums">
                    {formatUnix(row.created_at)}
                  </TableCell>
                  <TableCell className="text-right whitespace-nowrap">
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={() => setEditingExpiry(row)}
                      aria-label={t('apiKeys.editExpiryTitle')}
                      title={t('apiKeys.editExpiryTitle')}
                    >
                      <CalendarClockIcon className="size-4" />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="text-destructive hover:text-destructive"
                      onClick={() => setDeleting(row)}
                      aria-label={t('common.delete')}
                    >
                      <TrashIcon className="size-4" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>

          {filtered.length > PAGE_SIZE ? (
            <div className="flex items-center justify-between gap-2">
              <span className="text-sm text-muted-foreground">
                {t('apiKeys.pageInfo', { page: currentPage, pages: pageCount })}
              </span>
              <div className="flex items-center gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  disabled={currentPage <= 1}
                  onClick={() => setPage(currentPage - 1)}
                >
                  {t('common.prev')}
                </Button>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={currentPage >= pageCount}
                  onClick={() => setPage(currentPage + 1)}
                >
                  {t('common.next')}
                </Button>
              </div>
            </div>
          ) : null}
        </>
      )}

      <CreateApiKeyDialog
        key={`api-key-create-${createOpen}`}
        open={createOpen}
        onOpenChange={setCreateOpen}
        defaultRouteId={effectiveRoute}
        onCreated={(created) => setCreatedKey(created)}
      />

      <CreatedApiKeyDialog
        key={createdKey?.key ?? 'none'}
        apiKey={createdKey}
        onClose={() => setCreatedKey(null)}
      />

      <EditApiKeyExpiryDialog
        key={editingExpiry?.id ?? 'none'}
        apiKey={editingExpiry}
        onClose={() => setEditingExpiry(null)}
      />

      <ConfirmDialog
        open={Boolean(deleting)}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('apiKeys.deleteConfirmTitle')}
        description={t('apiKeys.deleteConfirmDesc')}
        onConfirm={confirmDelete}
        pending={deleteMutation.isPending}
      />
    </div>
  )
}
