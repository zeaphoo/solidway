import { CheckIcon, CopyIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useSession } from '@/hooks/useAuth'
import { useVersion } from '@/hooks/useAccount'
import { useIncidents } from '@/hooks/useIncidents'
import { useProviderHealth } from '@/hooks/useProviderHealth'
import { useAppInfo } from '@/hooks/useAppInfo'
import { useAccess } from '@/hooks/useSettings'
import { buildBaseUrls } from '@/lib/accessSnippets'
import { formatUnix } from '@/lib/format'

function Field({ label, value }) {
  return (
    <div className="space-y-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="font-mono text-xs break-all">{value || '—'}</dd>
    </div>
  )
}

/**
 * 「我的环境」：只读诊断卡。
 *
 * 报障时可直接截图或复制纯文本；不含任何写操作。
 * 监听地址与故障事件仅 admin 可见（普通用户调用 `/users` 之类会 403，也没必要看）。
 */
export default function DiagnosticsCard() {
  const { t } = useTranslation()
  const { data: session } = useSession()
  const isAdmin = session?.role === 'admin'
  // `/app/info` 仅 admin 可读；版本号走任意角色可见的 `/version`。
  const { data: versionInfo } = useVersion()
  const { data: appInfo } = useAppInfo(isAdmin)
  const { data: access } = useAccess()
  const { data: health = [] } = useProviderHealth()
  const { data: incidents = [] } = useIncidents({ state: 'open', limit: 5 }, isAdmin)
  const [copied, setCopied] = useState(false)

  const baseUrl = buildBaseUrls(access?.public_origins, `${window.location.origin}/v1`)[0] ?? ''
  const cooldown = health.filter((entry) => entry.state === 'cooldown').length
  const retired = health.filter((entry) => entry.state === 'retired').length

  const rows = [
    { key: 'version', label: t('help.diagnostics.version'), value: versionInfo?.version },
    ...(isAdmin
      ? [
          { key: 'mode', label: t('help.diagnostics.mode'), value: appInfo?.mode },
          { key: 'listen', label: t('help.diagnostics.listen'), value: appInfo?.listen?.effective },
        ]
      : []),
    { key: 'baseUrl', label: t('help.diagnostics.baseUrl'), value: baseUrl },
    { key: 'origin', label: t('help.diagnostics.origin'), value: window.location.origin },
    {
      key: 'health',
      label: t('help.diagnostics.health'),
      value: t('help.diagnostics.healthValue', { cooldown, retired }),
    },
  ]

  const copy = async () => {
    const text = [
      ...rows.map((row) => `${row.label}: ${row.value ?? ''}`),
      ...(isAdmin
        ? incidents.map(
            (incident) =>
              `- ${incident.route_name || incident.route_id} / ${incident.provider} · ${incident.kind} · ${formatUnix(incident.started_at)}`,
          )
        : []),
    ].join('\n')
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error(t('common.copyFailed'))
    }
  }

  return (
    <div className="space-y-4 rounded-lg border p-4">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <p className="text-xs text-muted-foreground">{t('help.diagnostics.hint')}</p>
        <Button variant="ghost" size="sm" onClick={copy}>
          {copied ? <CheckIcon className="size-4" /> : <CopyIcon className="size-4" />}
          {t('help.diagnostics.copy')}
        </Button>
      </div>

      <dl className="grid gap-3 sm:grid-cols-2">
        {rows.map((row) => (
          <Field key={row.key} label={row.label} value={row.value} />
        ))}
      </dl>

      <div className="space-y-1.5">
        <p className="text-xs text-muted-foreground">{t('help.diagnostics.healthHint')}</p>
        {health.length ? (
          <div className="flex flex-wrap gap-1.5">
            {health.map((entry) => (
              <Badge
                key={entry.provider_id}
                variant={entry.state === 'retired' ? 'destructive' : 'secondary'}
              >
                {entry.name || entry.provider} · {t(`health.${entry.state}`)}
              </Badge>
            ))}
          </div>
        ) : (
          <p className="text-xs text-emerald-600 dark:text-emerald-500">
            {t('help.diagnostics.healthEmpty')}
          </p>
        )}
      </div>

      {isAdmin ? (
        <div className="space-y-1.5">
          <p className="text-xs text-muted-foreground">{t('help.diagnostics.incidents')}</p>
          {incidents.length ? (
            <ul className="space-y-1 text-xs">
              {incidents.map((incident) => (
                <li key={incident.id} className="flex flex-wrap items-center gap-1.5">
                  <Badge variant={incident.severity === 'manual_action' ? 'destructive' : 'secondary'}>
                    {t(`incidents.severities.${incident.severity}`)}
                  </Badge>
                  <span className="font-mono">{incident.provider}</span>
                  <span className="text-muted-foreground">{incident.kind}</span>
                  <span className="text-muted-foreground tabular-nums">
                    {formatUnix(incident.started_at)}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-xs text-muted-foreground">{t('incidents.empty')}</p>
          )}
        </div>
      ) : null}
    </div>
  )
}
