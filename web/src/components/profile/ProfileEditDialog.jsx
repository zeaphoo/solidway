import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { useUpdateProfile } from '@/hooks/useAccount'
import { errorMessage } from '@/lib/format'
import { useSessionStore } from '@/stores/session'

/** 编辑昵称 / 头衔（对话框）：个人页只留身份卡，编辑按需打开。 */
export function ProfileEditDialog({ open, onOpenChange }) {
  const { t } = useTranslation()
  const user = useSessionStore((s) => s.user)
  const updateProfile = useUpdateProfile()
  const [displayName, setDisplayName] = useState('')
  const [title, setTitle] = useState('')

  // 每次打开都以当前值为准，避免上次取消留下的草稿。
  useEffect(() => {
    if (!open) return
    setDisplayName(user?.display_name ?? '')
    setTitle(user?.title ?? '')
  }, [open, user])

  const submit = (event) => {
    event.preventDefault()
    updateProfile.mutate(
      { display_name: displayName, title },
      {
        onSuccess: () => {
          toast.success(t('settings.profileSaved'))
          onOpenChange(false)
        },
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('settings.profileEditTitle')}</DialogTitle>
          <DialogDescription>{t('settings.profileHint')}</DialogDescription>
        </DialogHeader>

        <form onSubmit={submit} className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="display-name">{t('settings.displayName')}</Label>
            <Input
              id="display-name"
              autoFocus
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="profile-title">{t('settings.titleLabel')}</Label>
            <Input
              id="profile-title"
              value={title}
              onChange={(event) => setTitle(event.target.value)}
            />
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={updateProfile.isPending}>
              {updateProfile.isPending ? t('common.saving') : t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export default ProfileEditDialog
