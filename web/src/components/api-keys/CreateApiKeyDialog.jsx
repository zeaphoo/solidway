import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import ValidityPicker from '@/components/api-keys/ValidityPicker'
import { DEFAULT_VALIDITY, isValidityComplete, toExpiresAt } from '@/lib/apiKeyValidity'
import { useCreateApiKey } from '@/hooks/useApiKeys'
import { useRoutes } from '@/hooks/useRoutes'
import { errorMessage } from '@/lib/format'

/**
 * 创建 API Key 对话框。
 *
 * - `fixedRouteId`：在路由详情页内使用，路由固定不可选；
 * - `defaultRouteId`：全局页面通过 `?route=` 预选，但仍可切换；
 * - 父组件用 `key` 在打开时重挂载，保证每次都是干净状态。
 */
export default function CreateApiKeyDialog({
  open,
  onOpenChange,
  fixedRouteId,
  defaultRouteId,
  onCreated,
}) {
  const { t } = useTranslation()
  const createMutation = useCreateApiKey()
  const { data: routes } = useRoutes()
  const [routeId, setRouteId] = useState(fixedRouteId ?? defaultRouteId ?? '')
  const [name, setName] = useState('')
  const [validity, setValidity] = useState(DEFAULT_VALIDITY)

  // 可选路由恰好 1 条时默认选中并以纯文本展示；≥2 保持下拉，`fixedRouteId` 本就不渲染该字段。
  const routeOptions = routes ?? []
  const onlyRoute = routeOptions.length === 1 ? routeOptions[0] : null
  const targetRouteId = fixedRouteId ?? onlyRoute?.id ?? routeId

  const submit = () => {
    if (!targetRouteId) return
    createMutation.mutate(
      { routeId: targetRouteId, name, expires_at: toExpiresAt(validity) },
      {
        onSuccess: (created) => {
          onOpenChange(false)
          onCreated?.(created)
        },
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('apiKeys.new')}</DialogTitle>
          <DialogDescription>{t('apiKeys.subtitle')}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          {fixedRouteId ? null : onlyRoute ? (
            <div className="space-y-2">
              <Label>{t('apiKeys.route')}</Label>
              <p className="text-sm">{onlyRoute.name}</p>
            </div>
          ) : (
            <div className="space-y-2">
              <Label>{t('apiKeys.route')}</Label>
              <Select value={routeId || undefined} onValueChange={setRouteId}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder={t('apiKeys.selectRoute')} />
                </SelectTrigger>
                <SelectContent>
                  {routeOptions.map((route) => (
                    <SelectItem key={route.id} value={route.id}>
                      {route.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
          <div className="space-y-2">
            <Label htmlFor="api-key-name">
              {t('apiKeys.name')}{' '}
              <span className="text-muted-foreground">({t('common.optional')})</span>
            </Label>
            <Input id="api-key-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <ValidityPicker value={validity} onChange={setValidity} />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={submit}
            disabled={
              !targetRouteId || createMutation.isPending || !isValidityComplete(validity)
            }
          >
            {createMutation.isPending ? t('common.saving') : t('common.create')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
