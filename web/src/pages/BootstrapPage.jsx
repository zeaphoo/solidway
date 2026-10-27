import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate, useNavigate } from 'react-router'
import { toast } from 'sonner'

import LocaleToggle from '@/components/layout/LocaleToggle'
import ThemeToggle from '@/components/layout/ThemeToggle'
import Logo from '@/components/Logo'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useBootstrap, useBootstrapStatus } from '@/hooks/useAccount'
import { errorMessage } from '@/lib/format'

/**
 * 从 URL fragment 取引导令牌（`/bootstrap#<token>`）。
 * 读一次即入 state：清 fragment 后仍可用于提交。
 */
function readToken() {
  const raw = window.location.hash.replace(/^#/, '')
  return raw ? decodeURIComponent(raw) : ''
}

/** 清除 fragment，避免令牌留在地址栏 / 历史记录里。 */
function clearFragment() {
  window.history.replaceState(null, '', window.location.pathname)
}

export default function BootstrapPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const bootstrap = useBootstrap()
  const status = useBootstrapStatus()
  const [token] = useState(readToken)
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [failed, setFailed] = useState(false)

  const onSubmit = (event) => {
    event.preventDefault()
    if (password.length < 8) {
      toast.error(t('bootstrap.tooShort'))
      return
    }
    if (password !== confirm) {
      toast.error(t('bootstrap.mismatch'))
      return
    }
    bootstrap.mutate(
      { token, new_password: password },
      {
        onSuccess: () => {
          clearFragment()
          navigate('/routes', { replace: true })
        },
        onError: (error) => {
          setFailed(true)
          toast.error(errorMessage(t, error))
        },
      },
    )
  }

  // 已无待引导状态（引导完成或链接已过期）：直接去登录（已登录会被重定向进控制台）。
  if (status.isSuccess && !status.data?.pending) {
    return <Navigate to="/login" replace />
  }

  const body = () => {
    if (!token) {
      return (
        <div className="space-y-4">
          <p className="text-sm text-muted-foreground">{t('bootstrap.missing')}</p>
          <Button className="w-full" onClick={() => navigate('/login', { replace: true })}>
            {t('bootstrap.goLogin')}
          </Button>
        </div>
      )
    }

    if (failed) {
      return (
        <div className="space-y-4">
          <p className="text-sm text-muted-foreground">{t('bootstrap.invalid')}</p>
          <Button className="w-full" onClick={() => navigate('/login', { replace: true })}>
            {t('bootstrap.goLogin')}
          </Button>
        </div>
      )
    }

    return (
      <form onSubmit={onSubmit} className="space-y-4">
        <div className="space-y-2">
          <Label htmlFor="new-password">{t('bootstrap.newPassword')}</Label>
          <Input
            id="new-password"
            type="password"
            autoFocus
            autoComplete="new-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="confirm-password">{t('bootstrap.confirmPassword')}</Label>
          <Input
            id="confirm-password"
            type="password"
            autoComplete="new-password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
          />
        </div>
        <Button type="submit" className="w-full" disabled={bootstrap.isPending}>
          {bootstrap.isPending ? t('bootstrap.submitting') : t('bootstrap.submit')}
        </Button>
      </form>
    )
  }

  return (
    <div className="relative flex min-h-full items-center justify-center bg-background p-4">
      <div className="absolute top-4 right-4 flex items-center gap-1">
        <LocaleToggle />
        <ThemeToggle />
      </div>

      <div className="flex w-full max-w-sm flex-col items-center gap-6">
        <Logo size={32} withWordmark />
        <Card className="w-full">
          <CardHeader>
            <CardTitle>{t('bootstrap.title')}</CardTitle>
            <CardDescription>{t('bootstrap.subtitle')}</CardDescription>
          </CardHeader>
          <CardContent>{body()}</CardContent>
        </Card>
      </div>
    </div>
  )
}
