import { useState } from 'react'
import { zodResolver } from '@hookform/resolvers/zod'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { Navigate, useNavigate } from 'react-router'
import { toast } from 'sonner'
import { z } from 'zod'

import LocaleToggle from '@/components/layout/LocaleToggle'
import ThemeToggle from '@/components/layout/ThemeToggle'
import Logo from '@/components/Logo'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useLogin, useSession } from '@/hooks/useAuth'
import { isDesktopShell } from '@/lib/desktopLogin'
import { errorMessage } from '@/lib/format'

const schema = z.object({
  username: z.string().trim().min(1),
  password: z.string().min(1),
})

export default function LoginPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const login = useLogin()
  const session = useSession()
  // 只在壳的 webview 里提示；外部浏览器访问同一服务时看不到这句。
  const [desktopShell] = useState(isDesktopShell)

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm({ resolver: zodResolver(schema), defaultValues: { username: '', password: '' } })

  if (session.isSuccess) return <Navigate to="/routes" replace />

  const onSubmit = (values) => {
    login.mutate(values, {
      onSuccess: () => navigate('/routes', { replace: true }),
      onError: (error) => toast.error(errorMessage(t, error)),
    })
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
          <CardTitle>{t('auth.loginTitle')}</CardTitle>
          <CardDescription>{t('auth.loginSubtitle')}</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
            <div className="space-y-2">
              <Label htmlFor="username">{t('auth.username')}</Label>
              <Input id="username" autoFocus autoComplete="username" {...register('username')} />
              {errors.username ? (
                <p className="text-sm text-destructive">{t('auth.usernameRequired')}</p>
              ) : null}
            </div>

            <div className="space-y-2">
              <Label htmlFor="password">{t('auth.password')}</Label>
              <Input
                id="password"
                type="password"
                autoComplete="current-password"
                {...register('password')}
              />
              {errors.password ? (
                <p className="text-sm text-destructive">{t('auth.passwordRequired')}</p>
              ) : null}
            </div>

            <Button type="submit" className="w-full" disabled={login.isPending}>
              {login.isPending ? t('auth.signingIn') : t('auth.signIn')}
            </Button>
          </form>
        </CardContent>
      </Card>
      {desktopShell ? (
        <p className="text-center text-xs text-muted-foreground">{t('desktopLogin.hint')}</p>
      ) : null}
      </div>
    </div>
  )
}
