import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import AccessGuidePanel from '@/components/route/AccessGuidePanel'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useAccess } from '@/hooks/useSettings'
import { FORMATS, buildBaseUrls } from '@/lib/accessSnippets'

/**
 * 帮助中心的接入示例：无路由上下文，模型用占位符，格式并列展示。
 *
 * 示例代码与路由详情页由同一份 `AccessGuidePanel` / `lib/accessSnippets` 生成，
 * 避免两处内容漂移；接入地址与路由页共用 `buildBaseUrls`。
 */
export default function SnippetSection() {
  const { t } = useTranslation()
  const { data: access } = useAccess()
  const [selected, setSelected] = useState(0)

  const baseUrls = buildBaseUrls(access?.public_origins, `${window.location.origin}/v1`)
  const baseUrl = baseUrls[Math.min(selected, baseUrls.length - 1)] ?? ''

  return (
    <div className="space-y-3 rounded-lg border p-4">
      <div className="space-y-1">
        <p className="text-xs text-muted-foreground">{t('access.baseUrl')}</p>
        {baseUrls.length > 1 ? (
          <Select
            value={baseUrl}
            onValueChange={(value) => setSelected(baseUrls.indexOf(value))}
          >
            <SelectTrigger className="max-w-md">
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
          <code className="block truncate rounded-md border bg-muted px-2 py-1.5 font-mono text-xs">
            {baseUrl || '—'}
          </code>
        )}
      </div>

      <AccessGuidePanel baseUrl={baseUrl} formats={FORMATS} />
    </div>
  )
}
