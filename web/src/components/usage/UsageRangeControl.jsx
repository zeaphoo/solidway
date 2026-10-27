import { useTranslation } from 'react-i18next'

import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { USAGE_RANGES } from '@/lib/usage'

export default function UsageRangeControl({
  range,
  onRangeChange,
  from,
  to,
  onFromChange,
  onToChange,
  compact = false,
}) {
  const { t } = useTranslation()

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Select value={range} onValueChange={onRangeChange}>
        <SelectTrigger className={compact ? 'w-32' : 'w-36'}>
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {USAGE_RANGES.map((value) => (
            <SelectItem key={value} value={value}>
              {t(`usage.range_${value}`)}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      {range === 'custom' ? (
        <>
          <Input
            type="date"
            className={compact ? 'w-36' : 'w-40'}
            value={from ?? ''}
            onChange={(event) => onFromChange(event.target.value)}
            aria-label={t('usage.fromDate')}
          />
          <span className="text-sm text-muted-foreground">—</span>
          <Input
            type="date"
            className={compact ? 'w-36' : 'w-40'}
            value={to ?? ''}
            onChange={(event) => onToChange(event.target.value)}
            aria-label={t('usage.toDate')}
          />
        </>
      ) : null}
    </div>
  )
}
