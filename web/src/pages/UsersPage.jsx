import {
  ArchiveIcon,
  CheckIcon,
  CopyIcon,
  KeyRoundIcon,
  PencilIcon,
  PlusIcon,
  UserMinusIcon,
} from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import ConfirmDialog from '@/components/ConfirmDialog'
import { Badge } from '@/components/ui/badge'
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Skeleton } from '@/components/ui/skeleton'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import {
  useArchiveUser,
  useCreateUser,
  useOffboardUser,
  useResetUserPassword,
  useUpdateUser,
  useUsers,
} from '@/hooks/useUsers'
import { errorMessage, formatUnix } from '@/lib/format'
import { useSessionStore } from '@/stores/session'

const EMPTY_FORM = { username: '', display_name: '', title: '', role: 'user', password: '' }

function UserFormDialog({ open, onOpenChange, user, onCreatedPassword }) {
  const { t } = useTranslation()
  const isEdit = Boolean(user)
  const createMutation = useCreateUser()
  const updateMutation = useUpdateUser()
  const [form, setForm] = useState(EMPTY_FORM)

  const set = (key) => (event) => setForm((prev) => ({ ...prev, [key]: event.target.value }))

  // 每次打开时重置表单
  const [lastOpen, setLastOpen] = useState(false)
  if (open !== lastOpen) {
    setLastOpen(open)
    setForm(
      user
        ? {
            username: user.username,
            display_name: user.display_name ?? '',
            title: user.title ?? '',
            role: user.role,
            password: '',
          }
        : EMPTY_FORM,
    )
  }

  const mutation = isEdit ? updateMutation : createMutation
  const pending = mutation.isPending

  const submit = () => {
    const body = {
      display_name: form.display_name || undefined,
      title: form.title || undefined,
      role: form.role,
    }
    if (isEdit) {
      updateMutation.mutate(
        { id: user.id, body },
        {
          onSuccess: () => {
            toast.success(t('users.updated'))
            onOpenChange(false)
          },
          onError: (err) => toast.error(errorMessage(t, err)),
        },
      )
    } else {
      createMutation.mutate(
        { ...body, username: form.username, password: form.password || undefined },
        {
          onSuccess: (created) => {
            toast.success(t('users.created'))
            onOpenChange(false)
            if (created.password) onCreatedPassword(created.password)
          },
          onError: (err) => toast.error(errorMessage(t, err)),
        },
      )
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{isEdit ? t('users.editTitle') : t('users.createTitle')}</DialogTitle>
          <DialogDescription>{t('users.subtitle')}</DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          {!isEdit ? (
            <div className="space-y-2">
              <Label htmlFor="user-username">{t('users.username')}</Label>
              <Input id="user-username" value={form.username} onChange={set('username')} />
            </div>
          ) : null}
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-2">
              <Label htmlFor="user-display">{t('users.displayName')}</Label>
              <Input id="user-display" value={form.display_name} onChange={set('display_name')} />
            </div>
            <div className="space-y-2">
              <Label htmlFor="user-title">{t('users.titleLabel')}</Label>
              <Input id="user-title" value={form.title} onChange={set('title')} />
            </div>
          </div>
          <div className="space-y-2">
            <Label>{t('users.role')}</Label>
            <Select
              value={form.role}
              onValueChange={(value) => setForm((prev) => ({ ...prev, role: value }))}
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="user">{t('users.roleUser')}</SelectItem>
                <SelectItem value="admin">{t('users.roleAdmin')}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {!isEdit ? (
            <div className="space-y-2">
              <Label htmlFor="user-password">
                {t('users.password')}{' '}
                <span className="text-muted-foreground">({t('common.optional')})</span>
              </Label>
              <Input
                id="user-password"
                type="password"
                autoComplete="new-password"
                value={form.password}
                onChange={set('password')}
              />
              <p className="text-xs text-muted-foreground">{t('users.passwordHint')}</p>
            </div>
          ) : null}
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t('common.cancel')}
          </Button>
          <Button onClick={submit} disabled={pending || (!isEdit && !form.username.trim())}>
            {pending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function PasswordOnceDialog({ password, onClose }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(password)
      setCopied(true)
    } catch {
      toast.error(t('common.copyFailed'))
    }
  }

  return (
    <Dialog open={Boolean(password)} onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('users.passwordOnceTitle')}</DialogTitle>
          <DialogDescription className="text-destructive">
            {t('users.passwordOnceWarning')}
          </DialogDescription>
        </DialogHeader>
        <div className="flex items-center gap-2 rounded-md border bg-muted p-3">
          <code className="min-w-0 flex-1 truncate font-mono text-xs">{password}</code>
          <Button variant="ghost" size="icon-sm" onClick={copy} aria-label={t('common.copy')}>
            {copied ? <CheckIcon className="size-4" /> : <CopyIcon className="size-4" />}
          </Button>
        </div>
        <DialogFooter>
          <Button onClick={onClose}>{t('common.close')}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export default function UsersPage() {
  const { t } = useTranslation()
  const [showArchived, setShowArchived] = useState(false)
  const { data: users, isPending, isError, error } = useUsers({
    includeArchived: showArchived,
  })
  const updateMutation = useUpdateUser()
  const resetMutation = useResetUserPassword()
  const archiveMutation = useArchiveUser()
  const offboardMutation = useOffboardUser()
  const self = useSessionStore((s) => s.user)

  const [formOpen, setFormOpen] = useState(false)
  const [editing, setEditing] = useState(null)
  const [deleting, setDeleting] = useState(null)
  const [offboarding, setOffboarding] = useState(null)
  const [successor, setSuccessor] = useState('')
  const [oneTimePassword, setOneTimePassword] = useState(null)

  const openCreate = () => {
    setEditing(null)
    setFormOpen(true)
  }

  const openEdit = (user) => {
    setEditing(user)
    setFormOpen(true)
  }

  const toggleDisabled = (user, checked) => {
    updateMutation.mutate(
      { id: user.id, body: { disabled: checked } },
      { onError: (err) => toast.error(errorMessage(t, err)) },
    )
  }

  const resetPassword = (user) => {
    resetMutation.mutate(
      { id: user.id },
      {
        onSuccess: (result) => setOneTimePassword(result.password),
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  const confirmArchive = () => {
    if (!deleting) return
    archiveMutation.mutate(deleting.id, {
      onSuccess: () => {
        toast.success(t('users.archived'))
        setDeleting(null)
      },
      onError: (err) => toast.error(errorMessage(t, err)),
    })
  }

  const confirmOffboard = () => {
    if (!offboarding) return
    offboardMutation.mutate(
      { id: offboarding.id, transferTo: successor || undefined },
      {
        onSuccess: () => {
          toast.success(t('users.offboarded'))
          setOffboarding(null)
          setSuccessor('')
        },
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">{t('users.title')}</h1>
          <p className="text-sm text-muted-foreground">{t('users.subtitle')}</p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant={showArchived ? 'secondary' : 'outline'}
            onClick={() => setShowArchived((value) => !value)}
          >
            <ArchiveIcon className="size-4" />
            {t('users.showArchived')}
          </Button>
          <Button onClick={openCreate}>
            <PlusIcon className="size-4" />
            {t('users.new')}
          </Button>
        </div>
      </div>

      {isPending ? (
        <div className="space-y-2">
          <Skeleton className="h-9 w-full" />
          <Skeleton className="h-9 w-full" />
        </div>
      ) : isError ? (
        <p className="py-8 text-center text-sm text-destructive">{errorMessage(t, error)}</p>
      ) : !users?.length ? (
        <p className="py-16 text-center text-sm text-muted-foreground">{t('users.empty')}</p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t('users.username')}</TableHead>
              <TableHead>{t('users.displayName')}</TableHead>
              <TableHead>{t('users.titleLabel')}</TableHead>
              <TableHead>{t('users.role')}</TableHead>
              <TableHead>{t('providers.status')}</TableHead>
              <TableHead>{t('apiKeys.createdAt')}</TableHead>
              <TableHead className="text-right">{t('common.actions')}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {users.map((user) => {
              const isSelf = user.id === self?.id
              return (
                <TableRow key={user.id}>
                  <TableCell className="font-medium">
                    {user.username}
                    {isSelf ? (
                      <span className="ml-2 text-xs text-muted-foreground">({t('users.you')})</span>
                    ) : null}
                  </TableCell>
                  <TableCell>{user.display_name || '-'}</TableCell>
                  <TableCell className="text-muted-foreground">{user.title || '-'}</TableCell>
                  <TableCell>
                    <Badge variant={user.role === 'admin' ? 'default' : 'secondary'}>
                      {user.role === 'admin' ? t('users.roleAdmin') : t('users.roleUser')}
                    </Badge>
                  </TableCell>
                  <TableCell>
                    {user.archived_at ? (
                      <Badge variant="outline">{t('users.archivedBadge')}</Badge>
                    ) : (
                      <Switch
                        checked={user.disabled === 0}
                        disabled={
                          isSelf ||
                          (updateMutation.isPending && updateMutation.variables?.id === user.id)
                        }
                        onCheckedChange={(checked) => toggleDisabled(user, !checked)}
                        aria-label={t('providers.status')}
                      />
                    )}
                  </TableCell>
                  <TableCell className="text-muted-foreground tabular-nums">
                    {formatUnix(user.created_at)}
                  </TableCell>
                  <TableCell className="text-right whitespace-nowrap">
                    {user.archived_at ? null : (
                      <>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          onClick={() => openEdit(user)}
                          aria-label={t('common.edit')}
                        >
                          <PencilIcon className="size-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          onClick={() => resetPassword(user)}
                          aria-label={t('users.resetPassword')}
                          title={t('users.resetPassword')}
                        >
                          <KeyRoundIcon className="size-4" />
                        </Button>
                      </>
                    )}
                    {user.archived_at ? null : (
                      <>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          disabled={isSelf}
                          onClick={() => {
                            setOffboarding(user)
                            setSuccessor('')
                          }}
                          aria-label={t('users.offboard')}
                          title={t('users.offboard')}
                        >
                          <UserMinusIcon className="size-4" />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          className="text-destructive hover:text-destructive"
                          disabled={isSelf}
                          onClick={() => setDeleting(user)}
                          aria-label={t('users.archive')}
                          title={t('users.archive')}
                        >
                          <ArchiveIcon className="size-4" />
                        </Button>
                      </>
                    )}
                  </TableCell>
                </TableRow>
              )
            })}
          </TableBody>
        </Table>
      )}

      <UserFormDialog
        open={formOpen}
        onOpenChange={setFormOpen}
        user={editing}
        onCreatedPassword={setOneTimePassword}
      />

      <PasswordOnceDialog password={oneTimePassword} onClose={() => setOneTimePassword(null)} />

      <ConfirmDialog
        open={Boolean(deleting)}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('users.archiveConfirmTitle')}
        description={t('users.archiveConfirmDesc')}
        onConfirm={confirmArchive}
        pending={archiveMutation.isPending}
      />

      <Dialog
        open={Boolean(offboarding)}
        onOpenChange={(open) => {
          if (!open) {
            setOffboarding(null)
            setSuccessor('')
          }
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t('users.offboardTitle')}</DialogTitle>
            <DialogDescription>{t('users.offboardDesc')}</DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <Label>{t('users.successor')}</Label>
            <Select value={successor} onValueChange={setSuccessor}>
              <SelectTrigger className="w-full">
                <SelectValue placeholder={t('users.successorPlaceholder')} />
              </SelectTrigger>
              <SelectContent>
                {(users ?? [])
                  .filter(
                    (candidate) =>
                      !candidate.archived_at &&
                      candidate.disabled === 0 &&
                      candidate.id !== offboarding?.id &&
                      candidate.id !== self?.id,
                  )
                  .map((candidate) => (
                    <SelectItem key={candidate.id} value={candidate.id}>
                      {candidate.display_name || candidate.username}
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-muted-foreground">{t('users.offboardHint')}</p>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setOffboarding(null)}>
              {t('common.cancel')}
            </Button>
            <Button onClick={confirmOffboard} disabled={offboardMutation.isPending}>
              {offboardMutation.isPending ? t('common.saving') : t('users.offboardConfirm')}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}
