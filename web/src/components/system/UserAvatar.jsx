import { useTranslation } from 'react-i18next'

import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { avatarBackground, initialsFor } from '@/lib/avatar'
import { cn } from '@/lib/utils'

/** 尺寸：sm 用于侧栏 chip（24px）、md 用于菜单标题、lg 用于个人页身份区。 */
const SIZES = {
  sm: { box: 'size-6', text: 'text-[10px]' },
  md: { box: 'size-8', text: 'text-xs' },
  lg: { box: 'size-16', text: 'text-2xl' },
}

/**
 * 用户头像：传入 `src` 时显示图片，否则用「派生配色 + 首字」的默认头像。
 *
 * 配色种子固定取 `user.id`（改昵称不改色）；无 id 时回落登录名。
 */
export function UserAvatar({ user, src, size = 'md', className }) {
  const { t } = useTranslation()
  const name = user?.display_name || user?.username || '-'
  const seed = user?.id || user?.username || name
  const initials = initialsFor(user?.display_name, user?.username)
  const dims = SIZES[size] ?? SIZES.md

  return (
    <Avatar className={cn('ring-1 ring-black/5 dark:ring-white/10', dims.box, className)}>
      {src ? <AvatarImage src={src} alt={name} /> : null}
      <AvatarFallback
        role="img"
        aria-label={t('profile.avatarAlt', { name })}
        className={cn('font-semibold tracking-tight text-white select-none', dims.text)}
        style={{ backgroundImage: avatarBackground(seed) }}
      >
        {initials}
      </AvatarFallback>
    </Avatar>
  )
}
