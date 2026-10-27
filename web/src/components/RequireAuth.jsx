import { useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Navigate } from 'react-router'

import { Skeleton } from '@/components/ui/skeleton'
import { useSession } from '@/hooks/useAuth'
import { useDesktopLogin } from '@/hooks/useDesktopLogin'
import { readStoredToken } from '@/lib/desktopLogin'

export default function RequireAuth({ children }) {
  const { t } = useTranslation()
  const { isPending, isError } = useSession()
  const autoLogin = useDesktopLogin()
  const attempted = useRef(false)

  // 会话过期时（桌面模式）用记住的令牌自动换一次会话；成功后整页重载，
  // 让所有查询/缓存以新会话重新初始化。
  useEffect(() => {
    if (!isError || attempted.current) return
    const token = readStoredToken()
    if (!token) return
    attempted.current = true
    autoLogin.mutate(token, {
      onSuccess: () => window.location.replace(window.location.pathname + window.location.search),
    })
  }, [isError, autoLogin])

  if (isPending) {
    return (
      <div className="flex min-h-full flex-col items-center justify-center gap-3 text-muted-foreground">
        <Skeleton className="h-8 w-44" />
        <span className="text-sm">{t('common.loading')}</span>
      </div>
    )
  }

  if (isError) {
    // 有令牌且尚未恢复失败 → 先等自动恢复，不要闪一下登录页。
    if (readStoredToken() && !autoLogin.isError) {
      return (
        <div className="flex min-h-full flex-col items-center justify-center gap-3 text-muted-foreground">
          <Skeleton className="h-8 w-44" />
          <span className="text-sm">{t('common.loading')}</span>
        </div>
      )
    }
    return <Navigate to="/login" replace />
  }

  return children
}
