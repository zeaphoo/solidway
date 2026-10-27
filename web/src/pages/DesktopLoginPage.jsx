import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { useDesktopLogin } from '@/hooks/useDesktopLogin'
import {
  clearStoredToken,
  markDesktopShell,
  readStoredToken,
  storeToken,
  takeFragmentToken,
} from '@/lib/desktopLogin'

/**
 * 桌面自动登录页（公开路由 `/desktop#<token>`）。
 *
 * 壳在打开控制台时导航到这里；令牌换取会话后进入控制台。
 * 令牌存 sessionStorage：会话过期时 `RequireAuth` 会自动再换一次。
 */
export default function DesktopLoginPage() {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const login = useDesktopLogin()
  const started = useRef(false)

  // 优先用 URL fragment（壳每次导航都会带），其次用本会话记住的令牌。
  const [token] = useState(() => {
    const fromFragment = takeFragmentToken()
    if (fromFragment) {
      // 能带着 fragment 进来，说明是壳的 webview；外部浏览器不会走到这里。
      markDesktopShell()
      storeToken(fromFragment)
      return fromFragment
    }
    return readStoredToken()
  })

  useEffect(() => {
    if (started.current || !token) return undefined
    started.current = true
    login.mutate(token, {
      onSuccess: () => navigate('/routes', { replace: true }),
    })
    return undefined
  }, [token, login, navigate])

  const goLogin = () => {
    clearStoredToken()
    navigate('/login', { replace: true })
  }

  return (
    <div className="flex min-h-full items-center justify-center p-4">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>{t('desktopLogin.title')}</CardTitle>
          <CardDescription>
            {!token
              ? t('desktopLogin.missing')
              : login.isError
                ? t('desktopLogin.failed')
                : t('desktopLogin.signingIn')}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {!token || login.isError ? (
            <Button className="w-full" onClick={goLogin}>
              {t('desktopLogin.goLogin')}
            </Button>
          ) : (
            <div className="h-1 w-full overflow-hidden rounded bg-muted">
              <div className="h-full w-1/2 animate-pulse rounded bg-primary" />
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  )
}
