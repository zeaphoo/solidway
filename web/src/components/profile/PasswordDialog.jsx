import { useQueryClient } from '@tanstack/react-query'
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
import { useChangePassword } from '@/hooks/useAccount'
import { readStoredToken } from '@/lib/desktopLogin'
import { errorMessage } from '@/lib/format'

/**
 * 设置 / 修改口令（对话框）。
 *
 * - 桌面令牌会话（`passwordless`）：只填新口令，授权凭据是本机桌面令牌；
 * - 口令登录会话：需当前口令。
 *
 * 两种模式共用同一接口，差异只在请求体（ADR 0017）。
 */
export function PasswordDialog({ open, onOpenChange, passwordless }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const changePassword = useChangePassword()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')

  // 每次打开清空：口令不留在组件状态里。
  useEffect(() => {
    if (!open) return
    setCurrent('')
    setNext('')
    setConfirm('')
  }, [open])

  const submit = (event) => {
    event.preventDefault()
    if (next.length < 8) {
      toast.error(t('settings.passwordTooShort'))
      return
    }
    if (next !== confirm) {
      toast.error(t('settings.passwordMismatch'))
      return
    }
    const body = passwordless
      ? { new_password: next, desktop_token: readStoredToken() }
      : { current_password: current, new_password: next }
    changePassword.mutate(body, {
      onSuccess: () => {
        toast.success(t(passwordless ? 'settings.passwordSetDone' : 'settings.passwordChanged'))
        onOpenChange(false)
        // 让 has_password 立即翻转，口令卡状态无需刷新即可更新。
        queryClient.invalidateQueries({ queryKey: ['session'] })
      },
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>
            {t(passwordless ? 'settings.passwordSetAction' : 'settings.changePassword')}
          </DialogTitle>
          <DialogDescription>
            {t(passwordless ? 'settings.passwordSetHint' : 'settings.passwordHint')}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={submit} className="space-y-4">
          {passwordless ? null : (
            <div className="space-y-2">
              <Label htmlFor="current-password">{t('settings.currentPassword')}</Label>
              <Input
                id="current-password"
                type="password"
                autoComplete="current-password"
                autoFocus
                value={current}
                onChange={(event) => setCurrent(event.target.value)}
              />
            </div>
          )}

          <div className="space-y-2">
            <Label htmlFor="new-password">{t('settings.newPassword')}</Label>
            <Input
              id="new-password"
              type="password"
              autoComplete="new-password"
              autoFocus={passwordless}
              value={next}
              onChange={(event) => setNext(event.target.value)}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="confirm-password">{t('settings.confirmPassword')}</Label>
            <Input
              id="confirm-password"
              type="password"
              autoComplete="new-password"
              value={confirm}
              onChange={(event) => setConfirm(event.target.value)}
            />
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={changePassword.isPending}>
              {changePassword.isPending
                ? t('common.saving')
                : t(passwordless ? 'settings.passwordSetAction' : 'settings.changePassword')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export default PasswordDialog
