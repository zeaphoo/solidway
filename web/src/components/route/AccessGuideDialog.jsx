import { useTranslation } from 'react-i18next'

import AccessGuidePanel from '@/components/route/AccessGuidePanel'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

/**
 * 调用示例与终端 Agent 接入（对话框）。
 *
 * 内容由 `AccessGuidePanel` 承载，与帮助中心共用同一份实现，
 * 让路由详情页只留一行入口，不堆叠大段配置。
 */
export default function AccessGuideDialog({ open, onOpenChange, ...panelProps }) {
  const { t } = useTranslation()

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85vh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{t('access.guideTitle')}</DialogTitle>
          <DialogDescription>{t('access.guideHint')}</DialogDescription>
        </DialogHeader>

        <AccessGuidePanel {...panelProps} />
      </DialogContent>
    </Dialog>
  )
}
