import { Suspense, lazy } from 'react'
import { QueryClientProvider } from '@tanstack/react-query'
import { BrowserRouter, Navigate, Route, Routes } from 'react-router'

import RequireAdmin from '@/components/RequireAdmin'
import RequireAuth from '@/components/RequireAuth'
import AppShell from '@/components/layout/AppShell'
import { Skeleton } from '@/components/ui/skeleton'
import { Toaster } from '@/components/ui/sonner'
import { queryClient, setUnauthorizedHandler } from '@/lib/queryClient'
import { useSessionStore } from '@/stores/session'
import { useThemeStore } from '@/stores/theme'

// 页面按需加载：首屏只留壳与登录，其余页面（含图表库）各自成 chunk。
const ApiKeysPage = lazy(() => import('@/pages/ApiKeysPage'))
const BootstrapPage = lazy(() => import('@/pages/BootstrapPage'))
const DesktopLoginPage = lazy(() => import('@/pages/DesktopLoginPage'))
const GlobalUsagePage = lazy(() => import('@/pages/GlobalUsagePage'))
const HelpPage = lazy(() => import('@/pages/HelpPage'))
const LoginPage = lazy(() => import('@/pages/LoginPage'))
const OnboardingPage = lazy(() => import('@/pages/OnboardingPage'))
const ProviderIncidentsPage = lazy(() => import('@/pages/ProviderIncidentsPage'))
const RouteDetailPage = lazy(() => import('@/pages/RouteDetailPage'))
const RoutesIndex = lazy(() => import('@/pages/RoutesIndex'))
const SettingsPage = lazy(() => import('@/pages/SettingsPage'))
const SystemPage = lazy(() => import('@/pages/SystemPage'))
const UsagePage = lazy(() => import('@/pages/UsagePage'))
const UsersPage = lazy(() => import('@/pages/UsersPage'))

/** 页面懒加载时的占位（保持布局不跳动）。 */
function PageFallback() {
  return (
    <div className="mx-auto max-w-3xl space-y-3 py-6">
      <Skeleton className="h-7 w-40" />
      <Skeleton className="h-24 w-full" />
      <Skeleton className="h-24 w-full" />
    </div>
  )
}

// 全局 401：清会话并跳登录（同源，避免在登录页循环）。
setUnauthorizedHandler(() => {
  useSessionStore.getState().clear()
  if (window.location.pathname !== '/login') {
    window.location.assign('/login')
  }
})

function AppRoutes() {
  return (
    <Suspense fallback={<PageFallback />}>
      <Routes>
      <Route path="/login" element={<LoginPage />} />
      {/* 公开：admin 首次引导设置口令（令牌在 fragment 里） */}
      <Route path="/bootstrap" element={<BootstrapPage />} />
      {/* 公开：桌面模式自动登录（令牌在 fragment 里，由壳导航注入） */}
      <Route path="/desktop" element={<DesktopLoginPage />} />
      <Route
        element={
          <RequireAuth>
            <AppShell />
          </RequireAuth>
        }
      >
        <Route path="/" element={<Navigate to="/routes" replace />} />
        <Route path="/routes" element={<RoutesIndex />} />
        <Route path="/routes/:id" element={<RouteDetailPage />} />
        <Route path="/api-keys" element={<ApiKeysPage />} />
        <Route path="/welcome" element={<OnboardingPage />} />
        <Route path="/help" element={<HelpPage />} />
        <Route path="/usage" element={<UsagePage />} />
        <Route
          path="/usage/global"
          element={
            <RequireAdmin>
              <GlobalUsagePage />
            </RequireAdmin>
          }
        />
        <Route path="/profile" element={<SettingsPage />} />
        <Route path="/settings" element={<Navigate to="/profile" replace />} />
        <Route
          path="/system"
          element={
            <RequireAdmin>
              <SystemPage />
            </RequireAdmin>
          }
        />
        <Route
          path="/incidents"
          element={
            <RequireAdmin>
              <ProviderIncidentsPage />
            </RequireAdmin>
          }
        />
        <Route
          path="/users"
          element={
            <RequireAdmin>
              <UsersPage />
            </RequireAdmin>
          }
        />
      </Route>
        <Route path="*" element={<Navigate to="/routes" replace />} />
      </Routes>
    </Suspense>
  )
}

export default function App() {
  const theme = useThemeStore((s) => s.theme)

  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <AppRoutes />
      </BrowserRouter>
      <Toaster theme={theme} richColors position="top-center" />
    </QueryClientProvider>
  )
}
