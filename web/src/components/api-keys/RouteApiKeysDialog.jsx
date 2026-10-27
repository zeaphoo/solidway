import { CalendarClockIcon, TrashIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'
import { toast } from 'sonner'

import ConfirmDialog from '@/components/ConfirmDialog'
import ApiKeyExpiryCell from '@/components/api-keys/ApiKeyExpiryCell'
import EditApiKeyExpiryDialog from '@/components/api-keys/EditApiKeyExpiryDialog'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
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
import { useDeleteApiKey, useRouteApiKeys, useUpdateApiKey } from '@/hooks/useApiKeys'
import { errorMessage, formatUnix } from '@/lib/format'

/** 路由上下文内的 API Key 列表与启停/删除操作。 */
export default function RouteApiKeysDialog({ open, onOpenChange, routeId }) {
  const { t } = useTranslation()
  const { data: keys = [], isPending, isError, error } = useRouteApiKeys(routeId)
  const updateMutation = useUpdateApiKey()
  const deleteMutation = useDeleteApiKey()
  const [deleting, setDeleting] = useState(null)
  const [editingExpiry, setEditingExpiry] = useState(null)

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
    <>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{t('apiKeys.routeTitle')}</DialogTitle>
            <DialogDescription>{t('apiKeys.routeHint')}</DialogDescription>
          </DialogHeader>

          {isPending ? (
            <Skeleton className="h-24 w-full" />
          ) : isError ? (
            <p className="py-4 text-sm text-destructive">{errorMessage(t, error)}</p>
          ) : keys.length ? (
            <div className="max-h-[50vh] overflow-y-auto">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>{t('apiKeys.name')}</TableHead>
                    <TableHead>{t('apiKeys.masked')}</TableHead>
                    <TableHead>{t('apiKeys.expiresAt')}</TableHead>
                    <TableHead>{t('providers.status')}</TableHead>
                    <TableHead>{t('apiKeys.createdAt')}</TableHead>
                    <TableHead className="text-right">{t('common.actions')}</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {keys.map((row) => (
                    <TableRow key={row.id}>
                      <TableCell>{row.name || '-'}</TableCell>
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
            </div>
          ) : (
            <p className="py-8 text-center text-sm text-muted-foreground">{t('apiKeys.empty')}</p>
          )}

          <DialogFooter>
            <Button asChild variant="outline">
              <Link to={`/api-keys?route=${routeId}`}>{t('apiKeys.viewAll')}</Link>
            </Button>
            <Button onClick={() => onOpenChange(false)}>{t('common.close')}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

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
    </>
  )
}
