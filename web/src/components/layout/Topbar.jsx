import { CircleHelpIcon } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation } from 'react-router'

import LocaleToggle from '@/components/layout/LocaleToggle'
import ThemeToggle from '@/components/layout/ThemeToggle'
import AccessGuideDialog from '@/components/route/AccessGuideDialog'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useRoutes } from '@/hooks/useRoutes'
import { useAccess } from '@/hooks/useSettings'
import { FORMATS, buildBaseUrls } from '@/lib/accessSnippets'

function Crumb({ label, to }) {
  if (to) {
    return (
      <Link to={to} className="text-muted-foreground transition-colors hover:text-foreground">
        {label}
      </Link>
    )
  }
  return <span className="truncate font-medium">{label}</span>
}

export default function Topbar() {
  const { t } = useTranslation()
  const { pathname } = useLocation()
  const { data: routes } = useRoutes()
  const { data: access } = useAccess()
  const [guideOpen, setGuideOpen] = useState(false)

  const crumbs = []
  if (pathname.startsWith('/api-keys')) {
    crumbs.push({ label: t('nav.apiKeys') })
  } else if (pathname.startsWith('/usage')) {
    crumbs.push({ label: t('nav.usage') })
  } else if (pathname.startsWith('/welcome')) {
    crumbs.push({ label: t('nav.welcome') })
  } else if (pathname.startsWith('/help')) {
    crumbs.push({ label: t('nav.help') })
  } else if (pathname.startsWith('/profile') || pathname.startsWith('/settings')) {
    crumbs.push({ label: t('nav.profile') })
  } else if (pathname.startsWith('/system')) {
    crumbs.push({ label: t('nav.system') })
  } else if (pathname.startsWith('/incidents')) {
    crumbs.push({ label: t('nav.incidents') })
  } else if (pathname.startsWith('/users')) {
    crumbs.push({ label: t('nav.users') })
  } else {
    crumbs.push({ label: t('nav.routes'), to: '/routes' })
    const match = pathname.match(/^\/routes\/([^/]+)/)
    const current = match ? routes?.find((route) => route.id === match[1]) : null
    if (current) crumbs.push({ label: current.name })
  }

  // 帮助页的示例没有路由上下文：用对外接入地址 + 占位模型，格式并列展示。
  const baseUrls = buildBaseUrls(access?.public_origins, `${window.location.origin}/v1`)

  return (
    <>
      <header className="flex h-10 shrink-0 items-center justify-between gap-4 border-b px-6">
        <nav className="flex min-w-0 items-center gap-2 text-sm" aria-label="breadcrumb">
          {crumbs.map((crumb, index) => (
            <span key={`${crumb.label}-${index}`} className="flex min-w-0 items-center gap-2">
              {index > 0 ? <span className="text-muted-foreground">/</span> : null}
              <Crumb label={crumb.label} to={crumb.to} />
            </span>
          ))}
        </nav>

        <div className="flex shrink-0 items-center gap-1">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={t('nav.help')}
                title={t('nav.help')}
              >
                <CircleHelpIcon className="size-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-48">
              <DropdownMenuItem asChild>
                <Link to="/welcome">{t('nav.welcome')}</Link>
              </DropdownMenuItem>
              <DropdownMenuItem asChild>
                <Link to="/help">{t('nav.help')}</Link>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={() => setGuideOpen(true)}>
                {t('access.openGuide')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <LocaleToggle />
          <ThemeToggle />
        </div>
      </header>

      <AccessGuideDialog
        open={guideOpen}
        onOpenChange={setGuideOpen}
        baseUrl={baseUrls[0] ?? ''}
        formats={FORMATS}
      />
    </>
  )
}
