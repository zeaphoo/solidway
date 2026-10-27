import { useTranslation } from 'react-i18next'

import { errorMessage } from '@/lib/format'

/** 用量查询失败提示（R4-W-2）：失败时给出可读错误，不再停留在 skeleton / 空态。 */
export default function UsageQueryError({ error }) {
  const { t } = useTranslation()
  return (
    <p className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
      {errorMessage(t, error)}
    </p>
  )
}
