import { useEffect, useState } from 'react'
import { PlusIcon, TrashIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { SectionCard } from '@/components/ui/section-card'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import { useSystemInfo } from '@/hooks/useAccount'
import { useAppInfo, useRestartApp } from '@/hooks/useAppInfo'
import BindAddressPicker from '@/components/network/BindAddressPicker'
import { DesktopAppSection } from '@/components/system/DesktopAppSection'
import { useSettings, useTlsRetry, useTlsStatus, useUpdateSettings } from '@/hooks/useSettings'
import { request } from '@/lib/api'
import { errorMessage, formatUnixCompact } from '@/lib/format'

/** 只读运行信息。 */
function SystemInfoSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSystemInfo()

  const assets = data?.web_assets
  const rows = data
    ? [
        [t('settings.version'), data.version],
        [t('settings.webAssetsSource'), assets?.source ?? '-'],
        ...(assets?.path ? [[t('settings.webAssetsPath'), assets.path]] : []),
        [t('settings.webAssetsFiles'), String(assets?.files ?? 0)],
        ...(data.data_root ? [[t('settings.dataRoot'), data.data_root]] : []),
      ]
    : []

  return (
    <SectionCard title={t('settings.systemTitle')} description={t('settings.systemInfoHint')}>
      {isPending ? (
        <Skeleton className="h-32 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <dl className="grid gap-x-6 gap-y-3 sm:grid-cols-2">
          {rows.map(([label, value]) => (
            <div key={label} className="space-y-1">
              <dt className="text-xs text-muted-foreground">{label}</dt>
              <dd className="font-mono text-sm break-all">{value}</dd>
            </div>
          ))}
        </dl>
      )}
    </SectionCard>
  )
}

/** 把分钟偏移格式化为 `UTC+08:00`。 */
function formatUtcOffset(minutes) {
  const total = Number(minutes) || 0
  const sign = total < 0 ? '-' : '+'
  const abs = Math.abs(total)
  const hours = String(Math.floor(abs / 60)).padStart(2, '0')
  const rest = String(abs % 60).padStart(2, '0')
  return `UTC${sign}${hours}:${rest}`
}

/** 数据库驱动的系统设置（首个为用量统计）。 */
function SystemSettingsSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSettings(true)
  const update = useUpdateSettings()
  const [form, setForm] = useState(null)

  useEffect(() => {
    if (data?.usage) setForm({ ...data.usage })
  }, [data])

  const submit = (event) => {
    event.preventDefault()
    update.mutate(
      {
        usage: {
          enabled: form.enabled,
          detail_mode: form.detail_mode,
          detail_retention_hours: Number(form.detail_retention_hours),
          detail_sample_rate: Number(form.detail_sample_rate),
        },
      },
      {
        onSuccess: () => toast.success(t('settings.saved')),
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <SectionCard
      title={t('settings.usageTitle')}
      description={t('settings.systemSettingsHint')}
    >
      {isPending || !form ? (
        <Skeleton className="h-40 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <form onSubmit={submit} className="max-w-md space-y-4">
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <Label htmlFor="usage-enabled">{t('settings.usageEnabled')}</Label>
              <Switch
                id="usage-enabled"
                checked={form.enabled}
                onCheckedChange={(checked) => setForm((p) => ({ ...p, enabled: checked }))}
              />
            </div>

            <div className="space-y-2">
              <Label>{t('settings.detailMode')}</Label>
              <Select
                value={form.detail_mode}
                onValueChange={(value) => setForm((p) => ({ ...p, detail_mode: value }))}
              >
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="off">{t('settings.detailModeOff')}</SelectItem>
                  <SelectItem value="errors">{t('settings.detailModeErrors')}</SelectItem>
                  <SelectItem value="all">{t('settings.detailModeAll')}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-2">
                <Label htmlFor="detail-retention">{t('settings.detailRetention')}</Label>
                <Input
                  id="detail-retention"
                  type="number"
                  min="0"
                  max="8760"
                  value={form.detail_retention_hours}
                  onChange={(e) =>
                    setForm((p) => ({ ...p, detail_retention_hours: e.target.value }))
                  }
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="detail-sample">{t('settings.detailSampleRate')}</Label>
                <Input
                  id="detail-sample"
                  type="number"
                  min="0"
                  max="1"
                  step="0.1"
                  value={form.detail_sample_rate}
                  onChange={(e) => setForm((p) => ({ ...p, detail_sample_rate: e.target.value }))}
                />
              </div>
            </div>

            <div className="space-y-2">
              <Label>{t('settings.timezoneOffset')}</Label>
              <div className="flex items-center gap-2">
                <code className="rounded-md border bg-muted px-2 py-1.5 font-mono text-xs">
                  {formatUtcOffset(form.timezone_offset_minutes)}
                </code>
                <span className="text-xs text-muted-foreground">{t('settings.timezoneReadonly')}</span>
              </div>
              <p className="text-xs text-muted-foreground">{t('settings.timezoneOffsetHint')}</p>
            </div>
          </div>

          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </form>
      )}
    </SectionCard>
  )
}

const GATEWAY_FIELDS = [
  ['cooldown_auth_secs', 'settings.cooldownAuth'],
  ['cooldown_rate_limit_secs', 'settings.cooldownRateLimit'],
  ['cooldown_unavailable_secs', 'settings.cooldownUnavailable'],
  ['quota_cooldown_secs', 'settings.quotaCooldown'],
  ['affinity_ttl_secs', 'settings.affinityTtl'],
]

/** 转发链路运行期参数（即时下发给数据面）。 */
function GatewaySection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSettings(true)
  const update = useUpdateSettings()
  const [form, setForm] = useState(null)

  useEffect(() => {
    if (data?.gateway) setForm({ ...data.gateway })
  }, [data])

  const submit = (event) => {
    event.preventDefault()
    const gateway = {}
    for (const [key] of GATEWAY_FIELDS) gateway[key] = Number(form[key]) || 0
    update.mutate(
      { gateway },
      {
        onSuccess: () => toast.success(t('settings.saved')),
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <SectionCard title={t('settings.gatewayTitle')} description={t('settings.gatewayHint')}>
      {isPending || !form ? (
        <Skeleton className="h-32 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <form onSubmit={submit} className="max-w-md space-y-4">
          <div className="grid grid-cols-2 gap-4">
            {GATEWAY_FIELDS.map(([key, labelKey]) => (
              <div key={key} className="space-y-2">
                <Label htmlFor={`gw-${key}`}>{t(labelKey)}</Label>
                <Input
                  id={`gw-${key}`}
                  type="number"
                  min="0"
                  value={form[key]}
                  onChange={(e) => setForm((prev) => ({ ...prev, [key]: e.target.value }))}
                />
              </div>
            ))}
          </div>
          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </form>
      )}
    </SectionCard>
  )
}

const NETWORK_FIELDS = [
  ['port', 'settings.port', 'number', 'restart'],
  ['connect_timeout_secs', 'settings.connectTimeout', 'number', 'instant'],
  ['first_byte_timeout_secs', 'settings.firstByteTimeout', 'number', 'instant'],
]

/** 网络与上游超时。地址/端口需重启，超时即时生效。 */
function NetworkSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSettings(true)
  const appInfo = useAppInfo()
  const update = useUpdateSettings()
  const restart = useRestartApp()
  const [form, setForm] = useState(null)
  const [restarting, setRestarting] = useState(false)

  useEffect(() => {
    if (data?.network) setForm({ ...data.network })
  }, [data])

  // 重启后轮询直到服务恢复（本地服务，1s 足够）。
  useEffect(() => {
    if (!restarting) return undefined
    const timer = setInterval(async () => {
      try {
        const info = await request('/app/info')
        if (info?.listen) {
          setRestarting(false)
          toast.success(t('settings.restartDone'))
          window.location.reload()
        }
      } catch {
        /* 服务尚未恢复 */
      }
    }, 1000)
    return () => clearInterval(timer)
  }, [restarting, t])

  const listen = appInfo.data?.listen
  const overridden = listen?.source === 'command_line'
  const needsRestart = Boolean(listen?.restart_required)
  const canRestart = Boolean(appInfo.data?.can_restart)

  const submit = (event) => {
    event.preventDefault()
    const addresses = (form.bind_addrs ?? []).map((addr) => String(addr).trim()).filter(Boolean)
    if (!addresses.length) {
      toast.error(t('settings.bindAddrEmpty'))
      return
    }
    update.mutate(
      {
        network: {
          bind_addrs: addresses,
          // 兼容旧接口：首个地址同时写入单值字段。
          bind_addr: addresses[0],
          port: Number(form.port),
          connect_timeout_secs: Number(form.connect_timeout_secs),
          first_byte_timeout_secs: Number(form.first_byte_timeout_secs),
        },
      },
      {
        onSuccess: () => {
          toast.success(t('settings.saved'))
          appInfo.refetch()
        },
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  const doRestart = () => {
    restart.mutate(undefined, {
      onSuccess: () => setRestarting(true),
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  return (
    <SectionCard title={t('settings.networkTitle')} description={t('settings.networkHint')}>
      {isPending || !form ? (
        <Skeleton className="h-40 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <form onSubmit={submit} className="max-w-md space-y-4">
          {listen ? (
            <div className="flex flex-wrap items-center gap-x-6 gap-y-1 text-sm">
              <span className="text-muted-foreground">
                {t('settings.effectiveListen')}:{' '}
                <code className="font-mono text-foreground">{listen.effective}</code>
              </span>
              <span className="text-muted-foreground">
                {t('settings.listenSource')}:{' '}
                <span className="text-foreground">
                  {listen.source === 'command_line'
                    ? t('settings.sourceCommandLine')
                    : t('settings.sourceDatabase')}
                </span>
              </span>
            </div>
          ) : null}

          {overridden ? (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
              {t('settings.overriddenByCli')}
            </p>
          ) : null}

          <BindAddressPicker
            value={form.bind_addrs ?? []}
            onChange={(next) => setForm((prev) => ({ ...prev, bind_addrs: next }))}
          />

          <div className="grid grid-cols-2 gap-4">
            {NETWORK_FIELDS.map(([key, labelKey, type, effect]) => (
              <div key={key} className="space-y-2">
                <Label htmlFor={`net-${key}`}>{t(labelKey)}</Label>
                <Input
                  id={`net-${key}`}
                  type={type}
                  min={type === 'number' ? '1' : undefined}
                  value={form[key]}
                  onChange={(e) => setForm((prev) => ({ ...prev, [key]: e.target.value }))}
                />
                <p className="text-xs text-muted-foreground">
                  {effect === 'instant' ? t('settings.instantHint') : t('settings.restartHintLabel')}
                </p>
              </div>
            ))}
          </div>

          {needsRestart ? (
            <div className="flex flex-wrap items-center gap-3 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
              <span>
                {canRestart ? t('settings.restartRequired') : t('settings.restartHintStandalone')}
              </span>
              {canRestart ? (
                <Button type="button" size="sm" onClick={doRestart} disabled={restart.isPending || restarting}>
                  {restarting ? t('settings.restarting') : t('settings.restartNow')}
                </Button>
              ) : null}
            </div>
          ) : null}

          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </form>
      )}
    </SectionCard>
  )
}

/** 对外接入地址：客户端实际访问网关使用的 Base URL 列表。 */
function AccessSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSettings(true)
  const update = useUpdateSettings()
  const [draft, setDraft] = useState(null)

  const urls = draft ?? data?.access?.public_origins ?? []

  const setUrl = (index, value) => {
    const next = [...urls]
    next[index] = value
    setDraft(next)
  }

  const submit = (event) => {
    event.preventDefault()
    update.mutate(
      { access: { public_origins: urls.map((url) => url.trim()).filter(Boolean) } },
      {
        onSuccess: () => {
          setDraft(null)
          toast.success(t('settings.saved'))
        },
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <SectionCard title={t('settings.accessTitle')} description={t('settings.accessHint')}>
      {isPending && !draft ? (
        <Skeleton className="h-24 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <form onSubmit={submit} className="space-y-4">
          <div className="space-y-2">
            {urls.length ? (
              urls.map((url, index) => (
                <div key={index} className="flex items-center gap-2">
                  <Input
                    value={url}
                    placeholder={t('settings.accessUrlPlaceholder')}
                    onChange={(event) => setUrl(index, event.target.value)}
                  />
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => setDraft(urls.filter((_, item) => item !== index))}
                    title={t('settings.accessRemove')}
                    aria-label={t('settings.accessRemove')}
                  >
                    <TrashIcon className="size-4" />
                  </Button>
                </div>
              ))
            ) : (
              <p className="text-sm text-muted-foreground">{t('settings.accessEmpty')}</p>
            )}
            <Button type="button" variant="outline" size="sm" onClick={() => setDraft([...urls, ''])}>
              <PlusIcon className="size-4" />
              {t('settings.accessAdd')}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">{t('settings.accessDefaultHint')}</p>
          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </form>
      )}
    </SectionCard>
  )
}

/** 告警设置（Webhook）。 */
function AlertSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSettings(true)
  const update = useUpdateSettings()
  const [form, setForm] = useState(null)

  useEffect(() => {
    if (data?.alert) setForm({ ...data.alert })
  }, [data])

  const submit = (event) => {
    event.preventDefault()
    update.mutate(
      {
        alert: {
          webhook_url: form.webhook_url ?? '',
          notify_recovery: Boolean(form.notify_recovery),
          min_interval_minutes: Number(form.min_interval_minutes),
        },
      },
      {
        onSuccess: () => toast.success(t('settings.saved')),
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <SectionCard title={t('alerts.title')} description={t('alerts.hint')}>
      {isPending || !form ? (
        <Skeleton className="h-24 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <form onSubmit={submit} className="max-w-md space-y-4">
          <div className="space-y-2">
            <Label htmlFor="alert-webhook">{t('alerts.webhookUrl')}</Label>
            <Input
              id="alert-webhook"
              placeholder="https://hooks.example.com/solidway"
              value={form.webhook_url ?? ''}
              onChange={(e) => setForm((p) => ({ ...p, webhook_url: e.target.value }))}
            />
            <p className="text-xs text-muted-foreground">{t('alerts.webhookHint')}</p>
          </div>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="alert-interval">{t('alerts.minInterval')}</Label>
              <Input
                id="alert-interval"
                type="number"
                min="0"
                max="1440"
                value={form.min_interval_minutes}
                onChange={(e) =>
                  setForm((p) => ({ ...p, min_interval_minutes: e.target.value }))
                }
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="alert-recovery">{t('alerts.notifyRecovery')}</Label>
              <label className="flex h-9 items-center gap-2 text-sm">
                <Switch
                  id="alert-recovery"
                  checked={Boolean(form.notify_recovery)}
                  onCheckedChange={(checked) =>
                    setForm((p) => ({ ...p, notify_recovery: checked }))
                  }
                />
                <span className="text-muted-foreground">{t('alerts.notifyRecoveryHint')}</span>
              </label>
            </div>
          </div>
          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </form>
      )}
    </SectionCard>
  )
}

/** 内置 HTTPS / ACME：设置、证书状态与人工重试。 */
function TlsSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSettings(true)
  const status = useTlsStatus()
  const update = useUpdateSettings()
  const retry = useTlsRetry()
  const [form, setForm] = useState(null)
  const [restartRequired, setRestartRequired] = useState(false)

  useEffect(() => {
    if (data?.tls) setForm({ ...data.tls })
  }, [data])

  const cert = status.data?.cert
  const domains = status.data?.domains ?? []
  const skipped = status.data?.skipped ?? []
  const portHint = status.data?.challenge_port_hint

  const submit = (event) => {
    event.preventDefault()
    update.mutate(
      {
        tls: {
          enabled: Boolean(form.enabled),
          email: String(form.email ?? '').trim(),
          challenge: form.challenge,
          directory: form.directory,
          http_port: Number(form.http_port),
          https_port: Number(form.https_port),
          hsts: Boolean(form.hsts),
        },
      },
      {
        onSuccess: (result) => {
          setRestartRequired(Boolean(result?.restart_required))
          toast.success(t('settings.saved'))
        },
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  const stateBadge = (state) => {
    if (state === 'ok') return { label: t('tls.stateOk'), className: 'text-emerald-600 dark:text-emerald-500' }
    if (state === 'paused') return { label: t('tls.statePaused'), className: 'text-destructive' }
    return { label: t('tls.stateWaiting'), className: 'text-amber-600 dark:text-amber-500' }
  }

  return (
    <SectionCard title={t('tls.title')} description={t('tls.hint')}>
      {isPending || !form ? (
        <Skeleton className="h-40 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <form onSubmit={submit} className="space-y-4">
          <label className="flex items-center gap-2 text-sm">
            <Switch
              checked={Boolean(form.enabled)}
              onCheckedChange={(checked) => setForm((prev) => ({ ...prev, enabled: checked }))}
            />
            {t('tls.enabled')}
          </label>

          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="tls-email">{t('tls.email')}</Label>
              <Input
                id="tls-email"
                value={form.email ?? ''}
                placeholder="ops@example.com"
                onChange={(e) => setForm((prev) => ({ ...prev, email: e.target.value }))}
              />
            </div>
            <div className="space-y-2">
              <Label>{t('tls.challenge')}</Label>
              <Select
                value={form.challenge}
                onValueChange={(value) => setForm((prev) => ({ ...prev, challenge: value }))}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="http-01">HTTP-01</SelectItem>
                  <SelectItem value="tls-alpn-01">TLS-ALPN-01</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label>{t('tls.directory')}</Label>
              <Select
                value={form.directory}
                onValueChange={(value) => setForm((prev) => ({ ...prev, directory: value }))}
              >
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="production">{t('tls.directoryProduction')}</SelectItem>
                  <SelectItem value="staging">{t('tls.directoryStaging')}</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-2">
              <Label htmlFor="tls-http-port">{t('tls.httpPort')}</Label>
              <Input
                id="tls-http-port"
                type="number"
                min="1"
                max="65535"
                value={form.http_port}
                onChange={(e) => setForm((prev) => ({ ...prev, http_port: e.target.value }))}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="tls-https-port">{t('tls.httpsPort')}</Label>
              <Input
                id="tls-https-port"
                type="number"
                min="1"
                max="65535"
                value={form.https_port}
                onChange={(e) => setForm((prev) => ({ ...prev, https_port: e.target.value }))}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="tls-hsts">{t('tls.hsts')}</Label>
              <label className="flex h-9 items-center gap-2 text-sm">
                <Switch
                  id="tls-hsts"
                  checked={Boolean(form.hsts)}
                  onCheckedChange={(checked) => setForm((prev) => ({ ...prev, hsts: checked }))}
                />
                <span className="text-muted-foreground">{t('tls.hstsHint')}</span>
              </label>
            </div>
          </div>

          <p className="text-xs text-muted-foreground">{t('tls.portHint')}</p>

          {form.directory === 'staging' ? (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
              {t('tls.stagingWarning')}
            </p>
          ) : null}

          {portHint ? (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
              {portHint}
            </p>
          ) : null}

          {restartRequired ? (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-sm">
              {t('tls.restartRequired')}
            </p>
          ) : null}

          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>

          <div className="space-y-2 border-t pt-4">
            <div className="text-xs text-muted-foreground">{t('tls.domains')}</div>
            {domains.length ? (
              <div className="flex flex-wrap gap-1.5">
                {domains.map((domain) => (
                  <Badge key={domain} variant="secondary">
                    {domain}
                  </Badge>
                ))}
              </div>
            ) : (
              <p className="text-sm text-muted-foreground">{t('tls.noDomains')}</p>
            )}
            {skipped.length ? (
              <div className="space-y-1">
                {skipped.map((item) => (
                  <p key={item.origin} className="text-xs text-muted-foreground">
                    {t('tls.skipped', {
                      origin: item.origin,
                      reason: t(`tls.skipReasons.${item.reason}`, item.reason),
                    })}
                  </p>
                ))}
              </div>
            ) : null}

            <div className="flex flex-wrap items-center gap-x-6 gap-y-1 pt-2 text-sm">
              <span className="text-muted-foreground">
                {t('tls.certState')}:{' '}
                <span className={stateBadge(cert?.state).className}>
                  {stateBadge(cert?.state).label}
                </span>
              </span>
              <span className="text-muted-foreground">
                {t('tls.notAfter')}: {formatUnixCompact(cert?.not_after)}
              </span>
              <span className="text-muted-foreground">
                {t('tls.lastRenewed')}: {formatUnixCompact(cert?.last_renewed_at)}
              </span>
            </div>
            {cert?.last_error ? (
              <p className="text-xs text-destructive">{cert.last_error}</p>
            ) : null}
            {form.enabled ? (
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={retry.isPending}
                onClick={() =>
                  retry.mutate(undefined, {
                    onSuccess: (result) => {
                      if (result?.restart_required) toast.info(t('tls.retryNeedsRestart'))
                      else toast.success(t('tls.retryAccepted'))
                    },
                    onError: (err) => toast.error(errorMessage(t, err)),
                  })
                }
              >
                {retry.isPending ? t('tls.retrying') : t('tls.retry')}
              </Button>
            ) : null}
          </div>
        </form>
      )}
    </SectionCard>
  )
}

/** 会话有效期（`cookie_secure` 刻意不暴露，见后端注释）。 */
function SessionSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error } = useSettings(true)
  const update = useUpdateSettings()
  const [ttl, setTtl] = useState('')

  useEffect(() => {
    if (data?.auth) setTtl(String(data.auth.session_ttl_hours))
  }, [data])

  const submit = (event) => {
    event.preventDefault()
    update.mutate(
      { auth: { session_ttl_hours: Number(ttl) } },
      {
        onSuccess: () => toast.success(t('settings.saved')),
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <SectionCard title={t('settings.sessionTitle')} description={t('settings.sessionHint')}>
      {isPending || !ttl ? (
        <Skeleton className="h-24 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : (
        <form onSubmit={submit} className="max-w-md space-y-4">
          <div className="space-y-2">
            <Label htmlFor="session-ttl">{t('settings.sessionTtlHours')}</Label>
            <Input
              id="session-ttl"
              type="number"
              min="1"
              max="720"
              value={ttl}
              onChange={(event) => setTtl(event.target.value)}
            />
          </div>
          <Button type="submit" disabled={update.isPending}>
            {update.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </form>
      )}
    </SectionCard>
  )
}

export default function SystemPage() {
  const { t } = useTranslation()

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">{t('nav.system')}</h1>
        <p className="text-sm text-muted-foreground">{t('settings.systemHint')}</p>
      </div>
      <SystemInfoSection />
      <DesktopAppSection />
      <NetworkSection />
      <AccessSection />
      <TlsSection />
      <AlertSection />
      <SessionSection />
      <GatewaySection />
      <SystemSettingsSection />
    </div>
  )
}
