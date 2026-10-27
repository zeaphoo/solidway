import { CheckIcon, CopyIcon } from 'lucide-react'
import { useState } from 'react'
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

/** 明文只显示一次；父组件用 `key` 保证新 Key 重置复制状态。 */
export default function CreatedApiKeyDialog({ apiKey, onClose }) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(apiKey.key)
      setCopied(true)
    } catch {
      toast.error(t('common.copyFailed'))
    }
  }

  return (
    <Dialog open={Boolean(apiKey)} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('apiKeys.createdTitle')}</DialogTitle>
          <DialogDescription className="text-destructive">
            {t('apiKeys.createdWarning')}
          </DialogDescription>
        </DialogHeader>
        <div className="flex w-full min-w-0 items-start gap-2 rounded-md border bg-muted p-3">
          <code className="min-w-0 flex-1 break-all font-mono text-xs leading-relaxed">
            {apiKey?.key}
          </code>
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
