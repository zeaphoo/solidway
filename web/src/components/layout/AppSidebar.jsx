import {
  AlertTriangleIcon,
  BarChart3Icon,
  CircleHelpIcon,
  Globe2Icon,
  KeyRoundIcon,
  LogOutIcon,
  PlusIcon,
  RocketIcon,
  RouteIcon,
  ServerIcon,
  UserRoundIcon,
  UsersIcon,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { NavLink, useNavigate } from 'react-router'

import Logo from '@/components/Logo'
import { UserAvatar } from '@/components/system/UserAvatar'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { Skeleton } from '@/components/ui/skeleton'
import { useLogout } from '@/hooks/useAuth'
import { useVersion } from '@/hooks/useAccount'
import { useRoutes } from '@/hooks/useRoutes'
import { cn } from '@/lib/utils'
import { useSessionStore } from '@/stores/session'

/** 侧栏大类标题：小号大写 + 右侧延伸分隔线，用于区隔分组。 */
function Group({ label, first, children }) {
  return (
    <div className={first ? undefined : 'mt-5'}>
      <div className="flex items-center gap-2 px-2.5 pt-1 pb-1.5">
        <span className="text-[11px] font-semibold tracking-wider text-muted-foreground/80 uppercase select-none">
          {label}
        </span>
        <span className="h-px flex-1 bg-border" />
      </div>
      <div className="space-y-0.5">{children}</div>
    </div>
  )
}

function NavItem({ to, end, icon: Icon, label, action }) {
  return (
    <div className="relative">
      <NavLink
        to={to}
        end={end}
        className={({ isActive }) =>
          cn(
            'flex items-center gap-2 rounded-md px-2.5 py-1.5 text-sm transition-colors',
            action && 'pr-8',
            isActive
              ? 'bg-muted font-medium'
              : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground',
          )
        }
      >
        <Icon className="size-4 shrink-0" />
        <span className="truncate">{label}</span>
      </NavLink>
      {action ? <div className="absolute top-1/2 right-1 -translate-y-1/2">{action}</div> : null}
    </div>
  )
}

/** 侧栏最多直接列出的路由条数；超出部分由「查看全部」入口承载。 */
const MAX_SIDEBAR_ROUTES = 9

export default function AppSidebar({ onCreateRoute, isAdmin }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { data: routes, isPending } = useRoutes()
  const { data: version } = useVersion()
  const user = useSessionStore((s) => s.user)
  const logout = useLogout()

  const shown = (routes ?? []).slice(0, MAX_SIDEBAR_ROUTES)
  const hidden = Math.max((routes?.length ?? 0) - shown.length, 0)

  const handleLogout = () => {
    logout.mutate(undefined, { onSettled: () => navigate('/login', { replace: true }) })
  }

  return (
    <aside className="flex h-full w-64 shrink-0 flex-col border-r bg-sidebar">
      <div className="flex h-12 items-center gap-2 px-4">
        <Logo size={24} withWordmark />
        <span className="text-[11px] font-medium text-muted-foreground/80">
          {t('app.tagline')}
        </span>
      </div>

      <nav className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        <Group label={t('nav.groupManage')} first>
          <NavItem
            to="/routes"
            end
            icon={RouteIcon}
            label={t('nav.routes')}
            action={
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={onCreateRoute}
                aria-label={t('routes.new')}
                title={t('routes.new')}
              >
                <PlusIcon className="size-4" />
              </Button>
            }
          />
          <div className="space-y-0.5">
            {isPending ? (
              <div className="space-y-1 px-2.5 py-1">
                <Skeleton className="h-6 w-full" />
                <Skeleton className="h-6 w-full" />
              </div>
            ) : !routes?.length ? (
              <p className="px-2.5 py-1.5 pl-8 text-xs text-muted-foreground">—</p>
            ) : (
              shown.map((route) => (
                <NavLink
                  key={route.id}
                  to={`/routes/${route.id}`}
                  className={({ isActive }) =>
                    cn(
                      'flex items-center gap-2 rounded-md py-1.5 pr-2.5 pl-8 text-sm transition-colors',
                      isActive
                        ? 'bg-muted font-medium'
                        : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground',
                    )
                  }
                >
                  <span className="truncate">{route.name}</span>
                  {!route.enabled ? (
                    <span className="ml-auto shrink-0 rounded bg-background px-1.5 py-0.5 text-[10px] text-muted-foreground">
                      {t('common.disabled')}
                    </span>
                  ) : null}
                </NavLink>
              ))
            )}
            {hidden > 0 ? (
              <NavLink
                to="/routes"
                end
                className={({ isActive }) =>
                  cn(
                    'flex items-center gap-2 rounded-md py-1.5 pr-2.5 pl-8 text-sm transition-colors',
                    isActive
                      ? 'bg-muted font-medium'
                      : 'text-muted-foreground hover:bg-muted/60 hover:text-foreground',
                  )
                }
              >
                <span className="truncate">{t('routes.viewAll', { count: routes.length })}</span>
              </NavLink>
            ) : null}
          </div>
          <NavItem to="/api-keys" icon={KeyRoundIcon} label={t('nav.apiKeys')} />
        </Group>

        <Group label={t('nav.groupStats')}>
          <NavItem to="/usage" icon={BarChart3Icon} label={t('nav.usage')} />
          {isAdmin ? (
            <NavItem to="/usage/global" icon={Globe2Icon} label={t('nav.globalUsage')} />
          ) : null}
        </Group>

        <Group label={t('nav.groupSettings')}>
          <NavItem to="/profile" icon={UserRoundIcon} label={t('nav.profile')} />
          {isAdmin ? <NavItem to="/system" icon={ServerIcon} label={t('nav.system')} /> : null}
          {isAdmin ? (
            <NavItem
              to="/incidents"
              icon={AlertTriangleIcon}
              label={t('nav.incidents')}
            />
          ) : null}
          {isAdmin ? <NavItem to="/users" icon={UsersIcon} label={t('nav.users')} /> : null}
        </Group>

        <Group label={t('nav.groupHelp')}>
          <NavItem to="/welcome" icon={RocketIcon} label={t('nav.welcome')} />
          <NavItem to="/help" icon={CircleHelpIcon} label={t('nav.help')} />
        </Group>
      </nav>

      <div className="border-t p-2">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-sm transition-colors hover:bg-muted"
            >
              <UserAvatar user={user} size="sm" />
              <span className="truncate">
                {user?.display_name || user?.username || '-'}
              </span>
              {/* 版本号：非 admin 看不到「系统」页，这里对所有角色可见。 */}
              {version?.version ? (
                <span
                  className="ml-auto shrink-0 text-[11px] text-muted-foreground tabular-nums"
                  title={t('settings.version')}
                >
                  v{version.version}
                </span>
              ) : null}
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-56">
            <DropdownMenuLabel className="flex items-center gap-2">
              <UserAvatar user={user} size="md" />
              <span className="flex min-w-0 flex-col">
                <span className="truncate">{user?.display_name || user?.username || '-'}</span>
                {user?.title ? (
                  <span className="truncate text-xs font-normal text-muted-foreground">
                    {user.title}
                  </span>
                ) : null}
              </span>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={handleLogout} disabled={logout.isPending}>
              <LogOutIcon className="size-4" />
              {t('auth.logout')}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </aside>
  )
}
