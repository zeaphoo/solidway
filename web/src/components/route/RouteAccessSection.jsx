import { CheckIcon, CopyIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import CreateApiKeyDialog from '@/components/api-keys/CreateApiKeyDialog'
import CreatedApiKeyDialog from '@/components/api-keys/CreatedApiKeyDialog'
import AccessGuideDialog from '@/components/route/AccessGuideDialog'
import RouteApiKeysDialog from '@/components/api-keys/RouteApiKeysDialog'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { SectionCard } from '@/components/ui/section-card'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { useRouteApiKeys } from '@/hooks/useApiKeys'
import { useAccess } from '@/hooks/useSettings'
import { FORMATS, buildBaseUrls } from '@/lib/accessSnippets'

export default function RouteAccessSection({
  routeId,
  modelsByFormat,
  formats = [],
  readOnly = false,
}) {
  const { t } = useTranslation()
  const { data: keys = [], isPending } = useRouteApiKeys(routeId)
  const { data: access } = useAccess()
  const [guideOpen, setGuideOpen] = useState(false)
  const [copied, setCopied] = useState(null)
  const [selectedBaseUrl, setSelectedBaseUrl] = useState(0)
  const [createOpen, setCreateOpen] = useState(false)
  const [manageOpen, setManageOpen] = useState(false)
  const [createdKey, setCreatedKey] = useState(null)

  // 接入地址归一化与帮助页共用同一实现，避免两处展示不一致。
  const baseUrls = buildBaseUrls(access?.public_origins, `${window.location.origin}/v1`)
  const baseUrl = baseUrls[Math.min(selectedBaseUrl, baseUrls.length - 1)]
  const activeKey = keys.find((key) => key.status === 'active') ?? keys[0]
  const activeFormat = formats[0]
  const models = modelsByFormat?.[activeFormat] ?? []

  const copy = async (value, id) => {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(id)
      window.setTimeout(() => {
        setCopied((current) => (current === id ? null : current))
      }, 1500)
    } catch {
      toast.error(t('common.copyFailed'))
    }
  }

  const copyIcon = (id) =>
    copied === id ? <CheckIcon className="size-4" /> : <CopyIcon className="size-4" />

  return (
    <>
    <SectionCard title={t('access.title')} description={t('access.subtitle')}>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-1">
          <div className="text-xs text-muted-foreground">{t('access.baseUrl')}</div>
          <div className="flex items-center gap-2">
            {baseUrls.length > 1 ? (
              <Select
                value={baseUrl}
                onValueChange={(value) => setSelectedBaseUrl(baseUrls.indexOf(value))}
              >
                <SelectTrigger className="min-w-0 flex-1">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {baseUrls.map((url) => (
                    <SelectItem key={url} value={url}>
                      {url}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <code className="min-w-0 flex-1 truncate rounded-md border bg-muted px-2 py-1.5 font-mono text-xs">
                {baseUrl}
              </code>
            )}
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              onClick={() => copy(baseUrl, 'base-url')}
              title={t('common.copy')}
              aria-label={t('common.copy')}
            >
              {copyIcon('base-url')}
            </Button>
          </div>
        </div>

        <div className="space-y-1">
          <div className="text-xs text-muted-foreground">{t('access.apiKey')}</div>
          {isPending ? (
            <Skeleton className="h-8 w-full" />
          ) : activeKey ? (
            <div className="flex flex-wrap items-center gap-2">
              <code className="min-w-0 flex-1 truncate rounded-md border bg-muted px-2 py-1.5 font-mono text-xs">
                {activeKey.key_masked}
              </code>
              <Badge variant="secondary" className="tabular-nums">
                {t('access.keyCount', { count: keys.length })}
              </Badge>
              {readOnly ? null : (
                <>
                  <Button variant="outline" size="sm" onClick={() => setCreateOpen(true)}>
                    {t('apiKeys.new')}
                  </Button>
                  <Button variant="outline" size="sm" onClick={() => setManageOpen(true)}>
                    {t('access.manage')}
                  </Button>
                </>
              )}
            </div>
          ) : (
            <div className="flex items-center gap-2">
              <span className="flex-1 text-sm text-muted-foreground">{t('access.noKeys')}</span>
              {readOnly ? null : (
                <Button variant="outline" size="sm" onClick={() => setCreateOpen(true)}>
                  {t('access.create')}
                </Button>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="space-y-2">
        <div className="text-xs text-muted-foreground">{t('access.models')}</div>
        {models.length ? (
          <div className="flex max-h-28 flex-wrap gap-1.5 overflow-y-auto">
            {models.map((model) => (
              <Badge key={model} variant="secondary">
                {model}
              </Badge>
            ))}
          </div>
        ) : (
          <p className="text-sm text-muted-foreground">{t('access.noModels')}</p>
        )}
      </div>

      {formats.length ? (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-xs text-muted-foreground">{t('access.format')}</span>
            {FORMATS.filter((item) => formats.includes(item)).map((item) => (
              <Badge key={item} variant="secondary">
                {t(`providers.formats.${item}`)}
              </Badge>
            ))}
          </div>
          <Button type="button" variant="outline" size="sm" onClick={() => setGuideOpen(true)}>
            {t('access.openGuide')}
          </Button>
        </div>
      ) : (
        <p className="text-sm text-amber-600 dark:text-amber-500">
          {t('access.noFormats')}
        </p>
      )}
    </SectionCard>

    <AccessGuideDialog
      key={guideOpen ? 'guide-open' : 'guide-closed'}
      open={guideOpen}
      onOpenChange={setGuideOpen}
      baseUrl={baseUrl}
      modelsByFormat={modelsByFormat}
      formats={formats}
    />

    <CreateApiKeyDialog
      key={`route-key-create-${createOpen}`}
      open={createOpen}
      onOpenChange={setCreateOpen}
      fixedRouteId={routeId}
      onCreated={(created) => setCreatedKey(created)}
    />

    <CreatedApiKeyDialog
      key={createdKey?.key ?? 'none'}
      apiKey={createdKey}
      onClose={() => setCreatedKey(null)}
    />

    <RouteApiKeysDialog open={manageOpen} onOpenChange={setManageOpen} routeId={routeId} />
    </>
  )
}
