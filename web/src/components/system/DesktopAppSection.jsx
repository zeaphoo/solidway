import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { SectionCard } from '@/components/ui/section-card'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  useDesktopPreferences,
  useUpdateDesktopPreferences,
} from '@/hooks/useDesktopPreferences'
import { errorMessage } from '@/lib/format'

// 回执轮询：壳执行通常是毫秒级，400ms × 10 足够覆盖最慢的登录项写入。
const POLL_INTERVAL_MS = 400
const POLL_ATTEMPTS = 10

/**
 * 桌面应用设置（仅 admin，desktop 模式）。
 *
 * 开关是「意图」：提交后按钮进入「应用中」，等壳写回 `apply_id` 匹配的回执；
 * 失败时按回执把开关回滚到 `applied`（即文件里的现实值）。
 */
export function DesktopAppSection() {
  const { t } = useTranslation()
  const { data, isPending, isError, error, refetch } = useDesktopPreferences()
  const update = useUpdateDesktopPreferences()
  const [pending, setPending] = useState(null)
  const [draft, setDraft] = useState(null)

  const available = Boolean(data?.available)
  const prefs = data?.prefs
  const busy = Boolean(pending) || update.isPending

  // 没有等待中的提交时，界面跟随服务端读到的现实值（含托盘侧改动）。
  useEffect(() => {
    if (!pending && prefs) {
      setDraft({
        autostart: Boolean(prefs.autostart),
        close_window_to_tray: Boolean(prefs.close_window_to_tray),
      })
    }
  }, [pending, prefs])

  // 等回执：匹配 apply_id 后收尾；超时则提示并重新拉取。
  useEffect(() => {
    if (!pending) return undefined
    let attempts = 0
    const timer = setInterval(async () => {
      attempts += 1
      const result = await refetch()
      const status = result.data?.status
      if (status?.apply_id && status.apply_id === pending.applyId) {
        setPending(null)
        if (status.ok) {
          toast.success(t('desktop.applied'))
        } else {
          toast.error(status.error || t('desktop.applyFailed'))
        }
        return
      }
      if (attempts >= POLL_ATTEMPTS) {
        setPending(null)
        toast.warning(t('desktop.applyTimeout'))
        refetch()
      }
    }, POLL_INTERVAL_MS)
    return () => clearInterval(timer)
  }, [pending, refetch, t])

  const submit = (patch) => {
    if (!draft) return
    const body = { ...draft, ...patch }
    setDraft(body)
    update.mutate(body, {
      onSuccess: (result) => setPending({ applyId: result?.apply_id }),
      onError: (mutationError) => {
        toast.error(errorMessage(t, mutationError))
        refetch()
      },
    })
  }

  const status = data?.status

  return (
    <SectionCard title={t('desktop.title')} description={t('desktop.hint')}>
      {isPending ? (
        <Skeleton className="h-24 w-full" />
      ) : isError ? (
        <p className="text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : !available ? (
        <p className="text-sm text-muted-foreground">{t('desktop.unavailable')}</p>
      ) : (
        <div className="max-w-md space-y-3">
          <div className="flex items-center justify-between gap-4">
            <div className="space-y-0.5">
              <p className="text-sm font-medium">{t('desktop.autostart')}</p>
              <p className="text-xs text-muted-foreground">{t('desktop.autostartHint')}</p>
            </div>
            <Switch
              aria-label={t('desktop.autostart')}
              checked={Boolean(draft?.autostart)}
              disabled={busy}
              onCheckedChange={(checked) => submit({ autostart: checked })}
            />
          </div>

          <div className="flex items-center justify-between gap-4">
            <div className="space-y-0.5">
              <p className="text-sm font-medium">{t('desktop.closeToTray')}</p>
              <p className="text-xs text-muted-foreground">{t('desktop.closeToTrayHint')}</p>
            </div>
            <Switch
              aria-label={t('desktop.closeToTray')}
              checked={Boolean(draft?.close_window_to_tray)}
              disabled={busy}
              onCheckedChange={(checked) => submit({ close_window_to_tray: checked })}
            />
          </div>

          {pending ? (
            <p className="text-xs text-muted-foreground">{t('desktop.applying')}</p>
          ) : status && !status.ok && status.error ? (
            <p className="text-xs text-destructive">
              {t('desktop.lastError')}: {status.error}
            </p>
          ) : null}
        </div>
      )}
    </SectionCard>
  )
}
