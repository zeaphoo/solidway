import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { UserAvatar } from '@/components/system/UserAvatar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { SectionCard } from '@/components/ui/section-card'
import { PasswordDialog } from '@/components/profile/PasswordDialog'
import { ProfileEditDialog } from '@/components/profile/ProfileEditDialog'
import { isTokenSession } from '@/lib/desktopLogin'
import { useSessionStore } from '@/stores/session'

/** 身份区：头像 + 显示名 + 登录名 + 角色徽标 + 头衔，右上角是编辑入口。 */
function ProfileIdentitySection({ onEdit }) {
  const { t } = useTranslation()
  const user = useSessionStore((s) => s.user)

  if (!user) return null

  const name = user.display_name || user.username
  const roleLabel = user.role === 'admin' ? t('profile.roleAdmin') : t('profile.roleUser')

  return (
    <section className="flex items-start justify-between gap-4 rounded-lg border p-5">
      <div className="flex min-w-0 items-center gap-4">
        <UserAvatar user={user} size="lg" />
        <div className="min-w-0 space-y-1">
          <div className="flex flex-wrap items-center gap-2">
            <h2 className="truncate text-lg font-semibold">{name}</h2>
            <Badge variant={user.role === 'admin' ? 'default' : 'secondary'}>{roleLabel}</Badge>
          </div>
          <p className="truncate text-sm text-muted-foreground">@{user.username}</p>
          {user.title ? (
            <p className="truncate text-sm text-muted-foreground">{user.title}</p>
          ) : null}
        </div>
      </div>
      <Button variant="outline" size="sm" className="shrink-0" onClick={onEdit}>
        {t('settings.editProfile')}
      </Button>
    </section>
  )
}

/**
 * 口令卡：只展示状态与入口，具体操作在对话框里完成。
 *
 * 桌面令牌会话（免登录）不涉及口令，入口是「设置口令」，不要求当前口令；
 * 口令登录会话沿用「修改口令」。
 */
function PasswordSection({ passwordless, onOpen }) {
  const { t } = useTranslation()
  const user = useSessionStore((s) => s.user)
  const hasPassword = Boolean(user?.has_password)

  return (
    <SectionCard
      title={t('settings.passwordCardTitle')}
      description={t(passwordless ? 'settings.passwordSetHint' : 'settings.passwordHint')}
      action={
        <Button variant="outline" size="sm" onClick={onOpen}>
          {t(passwordless ? 'settings.passwordSetAction' : 'settings.changePassword')}
        </Button>
      }
    >
      {passwordless ? (
        <div className="space-y-3">
          <p className={hasPassword ? 'text-sm' : 'text-sm text-amber-600 dark:text-amber-500'}>
            {t(hasPassword ? 'settings.passwordStatusSet' : 'settings.passwordStatusUnset')}
          </p>
          <div className="space-y-1 border-t pt-3">
            <code className="block w-fit max-w-full overflow-x-auto rounded-md border bg-muted px-2 py-1.5 font-mono text-xs">
              solidway &lt;data_root&gt; --reset-password
            </code>
            <p className="text-xs text-muted-foreground">{t('settings.passwordDesktopCli')}</p>
          </div>
        </div>
      ) : null}
    </SectionCard>
  )
}

export default function ProfilePage() {
  const { t } = useTranslation()
  // 桌面令牌会话不涉及口令：给「设置口令」，而不是要求当前口令的「修改口令」。
  const passwordless = isTokenSession()
  const [profileOpen, setProfileOpen] = useState(false)
  const [passwordOpen, setPasswordOpen] = useState(false)

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <h1 className="text-xl font-semibold">{t('settings.title')}</h1>
      <ProfileIdentitySection onEdit={() => setProfileOpen(true)} />
      <PasswordSection passwordless={passwordless} onOpen={() => setPasswordOpen(true)} />

      <ProfileEditDialog open={profileOpen} onOpenChange={setProfileOpen} />
      <PasswordDialog
        open={passwordOpen}
        onOpenChange={setPasswordOpen}
        passwordless={passwordless}
      />
    </div>
  )
}
