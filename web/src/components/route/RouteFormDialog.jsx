import { zodResolver } from '@hookform/resolvers/zod'
import { useEffect, useState } from 'react'
import { Controller, useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { toast } from 'sonner'
import { z } from 'zod'

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
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import { useCreateRoute, useUpdateRoute } from '@/hooks/useRoutes'
import { errorMessage } from '@/lib/format'

const schema = z.object({
  name: z.string().trim().min(1),
  description: z.string().optional(),
  enabled: z.boolean(),
})

export default function RouteFormDialog({ open, onOpenChange, route }) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const isEdit = Boolean(route)
  const createMutation = useCreateRoute()
  const updateMutation = useUpdateRoute()
  const [serverError, setServerError] = useState(null)

  const {
    register,
    handleSubmit,
    reset,
    control,
    formState: { errors },
  } = useForm({
    resolver: zodResolver(schema),
    defaultValues: { name: '', description: '', enabled: true },
  })

  useEffect(() => {
    if (!open) return
    setServerError(null)
    reset({
      name: route?.name ?? '',
      description: route?.description ?? '',
      enabled: route ? Boolean(route.enabled) : true,
    })
  }, [open, route, reset])

  const mutation = isEdit ? updateMutation : createMutation

  const onSubmit = (values) => {
    const body = {
      name: values.name,
      description: values.description ? values.description : undefined,
      enabled: values.enabled,
    }
    mutation.mutate(isEdit ? { id: route.id, body } : body, {
      onSuccess: (data) => {
        toast.success(isEdit ? t('routes.updated') : t('routes.created'))
        onOpenChange(false)
        // 新建后直接进入详情页，省去用户在列表里再点一次。
        if (!isEdit && data?.id) navigate(`/routes/${data.id}`)
      },
      onError: (error) => {
        if (error?.code === 'conflict') setServerError(t('errors.conflict'))
        else toast.error(errorMessage(t, error))
      },
    })
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit(onSubmit)} className="space-y-4">
          <DialogHeader>
            <DialogTitle>{isEdit ? t('routes.edit') : t('routes.new')}</DialogTitle>
            <DialogDescription>{t('routes.subtitle')}</DialogDescription>
          </DialogHeader>

          <div className="space-y-2">
            <Label htmlFor="route-name">{t('routes.name')}</Label>
            <Input id="route-name" autoFocus {...register('name')} />
            {errors.name || serverError ? (
              <p className="text-sm text-destructive">{serverError ?? t('routes.nameRequired')}</p>
            ) : null}
          </div>

          <div className="space-y-2">
            <Label htmlFor="route-description">
              {t('routes.description')}{' '}
              <span className="text-muted-foreground">({t('common.optional')})</span>
            </Label>
            <Textarea
              id="route-description"
              rows={3}
              className="min-h-20 font-sans"
              {...register('description')}
            />
          </div>

          <div className="flex items-center justify-between rounded-lg border p-3">
            <Label htmlFor="route-enabled">{t('common.enabled')}</Label>
            <Controller
              control={control}
              name="enabled"
              render={({ field }) => (
                <Switch
                  id="route-enabled"
                  checked={field.value}
                  onCheckedChange={field.onChange}
                />
              )}
            />
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? t('common.saving') : t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
