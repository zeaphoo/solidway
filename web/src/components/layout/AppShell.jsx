import { useEffect, useRef, useState } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router'

import AppSidebar from '@/components/layout/AppSidebar'
import Topbar from '@/components/layout/Topbar'
import RouteFormDialog from '@/components/route/RouteFormDialog'
import { useSession } from '@/hooks/useAuth'
import { useRoutes } from '@/hooks/useRoutes'
import { shouldAutoEnter } from '@/lib/onboardingSteps'
import { readOnboarding } from '@/stores/onboarding'

export default function AppShell() {
  const { data: user } = useSession()
  const { data: routes } = useRoutes()
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const [routeFormOpen, setRouteFormOpen] = useState(false)
  const [editingRoute, setEditingRoute] = useState(null)

  // 首次登录引导：仅在「落在路由总览」且「没有任何路由」时强制进入一次。
  // 用 ref 做闸门，保证用户主动离开后不会被弹回（每次加载页只判一次）。
  const autoEntered = useRef(false)
  useEffect(() => {
    if (autoEntered.current || !user || !routes) return
    autoEntered.current = true
    if (pathname !== '/routes' && pathname !== '/') return
    const dismissed = readOnboarding(user.id).dismissed
    if (!shouldAutoEnter({ routeCount: routes.length, dismissed })) return
    navigate('/welcome', { replace: true })
  }, [user, routes, pathname, navigate])

  const openCreateRoute = () => {
    setEditingRoute(null)
    setRouteFormOpen(true)
  }

  const openEditRoute = (route) => {
    setEditingRoute(route)
    setRouteFormOpen(true)
  }

  return (
    <div className="flex h-full overflow-hidden bg-background">
      <AppSidebar onCreateRoute={openCreateRoute} isAdmin={user?.role === 'admin'} />

      <div className="flex min-h-0 min-w-0 flex-1 flex-col">
        <Topbar />
        <main className="min-h-0 flex-1 overflow-y-auto px-6 py-6">
          <Outlet context={{ openCreateRoute, openEditRoute }} />
        </main>
      </div>

      <RouteFormDialog open={routeFormOpen} onOpenChange={setRouteFormOpen} route={editingRoute} />
    </div>
  )
}
