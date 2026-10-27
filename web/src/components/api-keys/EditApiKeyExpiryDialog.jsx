import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import ValidityPicker from '@/components/api-keys/ValidityPicker'
import { DEFAULT_VALIDITY, isValidityComplete, toExpiresAt } from '@/lib/apiKeyValidity'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useUpdateApiKey } from '@/hooks/useApiKeys'
import { errorMessage, formatUnix } from '@/lib/format'

/** 修改某个 Key 的到期时间（延长 / 设为永不过期 / 自定义日期）。 */
export default function EditApiKeyExpiryDialog({ apiKey, onClose }) {
  const { t } = useTranslation()
  const updateMutation = useUpdateApiKey()
  const [validity, setValidity] = useState({ ...DEFAULT_VALIDITY, mode: '30' })

  const save = () => {
    if (!apiKey || !isValidityComplete(validity)) return
    updateMutation.mutate(
      { id: apiKey.id, body: { expires_at: toExpiresAt(validity) } },
      {
        onSuccess: () => {
          toast.success(t('apiKeys.expiryUpdated'))
          onClose()
        },
        onError: (err) => toast.error(errorMessage(t, err)),
      },
    )
  }

  return (
    <Dialog open={Boolean(apiKey)} onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t('apiKeys.editExpiryTitle')}</DialogTitle>
          <DialogDescription>
            {t('apiKeys.currentExpiry', {
              value: apiKey?.expires_at
                ? formatUnix(apiKey.expires_at)
                : t('apiKeys.never'),
            })}
          </DialogDescription>
        </DialogHeader>
        <ValidityPicker value={validity} onChange={setValidity} />
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button
            onClick={save}
            disabled={updateMutation.isPending || !isValidityComplete(validity)}
          >
            {updateMutation.isPending ? t('common.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
