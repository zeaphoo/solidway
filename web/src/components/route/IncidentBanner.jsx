import { AlertTriangleIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { useIncidents } from '@/hooks/useIncidents'

/**
 * 路由详情顶部横幅：该路由下存在未解决故障时提示。
 *
 * 只读展示；解除入口在系统页（仅 admin）。
 */
export default function IncidentBanner({ routeId }) {
  const { t } = useTranslation()
  const { data = [] } = useIncidents(
    { route_id: routeId, state: 'open', limit: 20 },
    // 路由详情尚未加载时不发请求，避免查询成"全站故障"。
    Boolean(routeId),
  )
  if (!data.length) return null

  const manual = data.filter((row) => row.severity === 'manual_action').length
  return (
    <div className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
      <AlertTriangleIcon className="mt-0.5 size-4 shrink-0 text-amber-600 dark:text-amber-500" />
      <div className="space-y-0.5">
        <div className="font-medium">
          {t('incidents.banner', { count: data.length })}
        </div>
        {manual ? (
          <div className="text-xs text-muted-foreground">
            {t('incidents.bannerManual', { count: manual })}
          </div>
        ) : null}
      </div>
    </div>
  )
}
