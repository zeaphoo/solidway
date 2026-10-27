import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { expiryState } from '@/lib/apiKeyValidity'
import { formatUnix } from '@/lib/format'

/** Key 到期展示：永不 / 已过期 / 即将过期（7 天内）/ 正常日期。 */
export default function ApiKeyExpiryCell({ expiresAt }) {
  const { t } = useTranslation()
  if (!expiresAt) {
    return <span className="text-muted-foreground">{t('apiKeys.never')}</span>
  }

  const state = expiryState(expiresAt)
  if (state === 'expired') {
    return (
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge variant="destructive">{t('apiKeys.expired')}</Badge>
        <span className="text-xs text-muted-foreground tabular-nums">
          {formatUnix(expiresAt)}
        </span>
      </div>
    )
  }

  if (state === 'soon') {
    return (
      <div className="flex flex-wrap items-center gap-1.5">
        <Badge variant="secondary">{t('apiKeys.expiringSoon')}</Badge>
        <span className="text-xs text-muted-foreground tabular-nums">
          {formatUnix(expiresAt)}
        </span>
      </div>
    )
  }

  return (
    <span className="text-muted-foreground tabular-nums">{formatUnix(expiresAt)}</span>
  )
}
