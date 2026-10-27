import { ArrowRightIcon, KeyRoundIcon, LayersIcon, RouteIcon, SparklesIcon } from 'lucide-react'
import { useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate, useOutletContext } from 'react-router'

import ProgressBar from '@/components/onboarding/ProgressBar'
import StepCard from '@/components/onboarding/StepCard'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import { useApiKeys } from '@/hooks/useApiKeys'
import { useSession } from '@/hooks/useAuth'
import { useProviderKeys } from '@/hooks/useProviderKeys'
import { useRoutes } from '@/hooks/useRoutes'
import { useAccess, useSettings } from '@/hooks/useSettings'
import { useUsageSummary } from '@/hooks/useUsage'
import { useUsers } from '@/hooks/useUsers'
import { buildContext, evaluateProgress, onboardingSteps } from '@/lib/onboardingSteps'
import { markOnboardingDone } from '@/stores/onboarding'

/** 概念导语：不计入进度，只解释三者的关系，避免第一次进来的人没有坐标系。 */
function ConceptCard() {
  const { t } = useTranslation()
  const items = [
    { key: 'route', icon: RouteIcon },
    { key: 'provider', icon: LayersIcon },
    { key: 'apiKey', icon: KeyRoundIcon },
  ]

  return (
    <section className="space-y-3 rounded-lg border bg-muted/30 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <SparklesIcon className="size-4 text-muted-foreground" />
          <h2 className="text-sm font-medium">{t('onboarding.concepts.title')}</h2>
        </div>
        <Link
          to="/help#concepts"
          className="inline-flex items-center gap-1 text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
        >
          {t('onboarding.concepts.more')}
          <ArrowRightIcon className="size-3" />
        </Link>
      </div>
      <div className="grid gap-3 sm:grid-cols-3">
        {items.map(({ key, icon: Icon }) => (
          <div key={key} className="space-y-1">
            <div className="flex items-center gap-1.5 text-xs font-medium">
              <Icon className="size-3.5 text-muted-foreground" />
              {t(`onboarding.concepts.${key}.title`)}
            </div>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {t(`onboarding.concepts.${key}.desc`)}
            </p>
          </div>
        ))}
      </div>
    </section>
  )
}

export default function OnboardingPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { openCreateRoute } = useOutletContext()

  const { data: session } = useSession()
  const isAdmin = session?.role === 'admin'

  const routesQuery = useRoutes()
  const apiKeysQuery = useApiKeys()
  const sampleRouteId = routesQuery.data?.[0]?.id
  const providerKeysQuery = useProviderKeys(sampleRouteId)
  const usageQuery = useUsageSummary({ range: '30d' })
  const accessQuery = useAccess()
  // admin 专属数据：非 admin 不发请求（/users、/settings 会 403）。
  const usersQuery = useUsers({ enabled: isAdmin })
  const settingsQuery = useSettings(isAdmin)

  const steps = useMemo(() => onboardingSteps(session?.role), [session?.role])
  const ctx = useMemo(
    () =>
      buildContext({
        routes: routesQuery.data,
        apiKeys: apiKeysQuery.data,
        providerKeys: providerKeysQuery.data,
        usage: usageQuery.data,
        access: accessQuery.data,
        users: usersQuery.data,
        settings: settingsQuery.data,
      }),
    [
      routesQuery.data,
      apiKeysQuery.data,
      providerKeysQuery.data,
      usageQuery.data,
      accessQuery.data,
      usersQuery.data,
      settingsQuery.data,
    ],
  )

  // 路由与 Key 未就绪时进度不可信：只显示骨架，不显示 0%。
  const pending = routesQuery.isPending || apiKeysQuery.isPending
  const progress = useMemo(() => evaluateProgress(steps, ctx, pending), [steps, ctx, pending])
  const setupRows = progress.rows.filter((row) => row.group === 'setup')
  const opsRows = progress.rows.filter((row) => row.group === 'ops')

  // 「完成」只由接入组决定：运维组是提醒（可稍后完成），不阻塞完成态与进度条。
  const setupProgress = useMemo(
    () => evaluateProgress(steps.filter((step) => step.group === 'setup'), ctx, pending),
    [steps, ctx, pending],
  )
  const opsDone = opsRows.filter((row) => row.done).length

  const userId = session?.id

  // 接入跑通即写本机标记：之后不再自动进入（入口仍在）。
  useEffect(() => {
    if (pending || !userId || !setupProgress.complete) return
    markOnboardingDone(userId, 'completed')
  }, [pending, userId, setupProgress.complete])

  const handleAction = (step) => {
    const action = step.action
    if (!action) return
    if (action.kind === 'dialog' && action.dialog === 'route') {
      openCreateRoute()
      return
    }
    if (action.kind === 'link') navigate(action.to)
  }

  const handleSkip = () => {
    if (userId) markOnboardingDone(userId, 'skipped')
    navigate('/routes', { replace: true })
  }

  const complete = setupProgress.complete && !pending

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="space-y-1">
          <h1 className="text-xl font-semibold">{t('onboarding.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('onboarding.subtitle')}</p>
        </div>
        <Button variant="ghost" size="sm" onClick={handleSkip}>
          {t('onboarding.skip')}
        </Button>
      </div>

      {pending ? (
        <Skeleton className="h-10 w-full" />
      ) : complete ? (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-emerald-600/30 bg-emerald-500/10 p-4">
          <div className="space-y-1">
            <p className="text-sm font-medium">{t('onboarding.completeTitle')}</p>
            <p className="text-xs text-muted-foreground">{t('onboarding.completeDesc')}</p>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" asChild>
              <Link to="/usage">{t('onboarding.goUsage')}</Link>
            </Button>
            <Button size="sm" asChild>
              <Link to="/help">{t('onboarding.goHelp')}</Link>
            </Button>
          </div>
        </div>
      ) : (
        <ProgressBar
          percent={setupProgress.percent}
          label={t('onboarding.progress', {
            done: setupProgress.done,
            total: setupProgress.total,
          })}
        />
      )}

      <ConceptCard />

      <section className="space-y-2">
        <h2 className="text-xs font-semibold tracking-wider text-muted-foreground uppercase">
          {t('onboarding.groups.setup')}
        </h2>
        {pending ? (
          <div className="space-y-2">
            <Skeleton className="h-20 w-full" />
            <Skeleton className="h-20 w-full" />
          </div>
        ) : (
          <div className="space-y-2">
            {setupRows.map((row, index) => (
              <StepCard
                key={row.id}
                index={index}
                step={row}
                isNext={row.id === progress.nextStepId}
                onAction={handleAction}
              />
            ))}
          </div>
        )}
      </section>

      {isAdmin && !pending ? (
        <section className="space-y-2">
          <div className="flex flex-wrap items-baseline gap-2">
            <h2 className="text-xs font-semibold tracking-wider text-muted-foreground uppercase">
              {t('onboarding.groups.ops')}
            </h2>
            <span className="text-[11px] text-muted-foreground">
              {t('onboarding.groups.opsHint')} · {t('onboarding.groups.opsProgress', {
                done: opsDone,
                total: opsRows.length,
              })}
            </span>
          </div>
          <div className="space-y-2">
            {opsRows.map((row, index) => (
              <StepCard
                key={row.id}
                index={index}
                step={row}
                isNext={row.id === progress.nextStepId}
                onAction={handleAction}
              />
            ))}
          </div>
        </section>
      ) : null}
    </div>
  )
}
