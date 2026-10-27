import { useTranslation } from 'react-i18next'

import { todayIsoDate } from '@/lib/apiKeyValidity'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

export default function ValidityPicker({ value, onChange }) {
  const { t } = useTranslation()
  const today = todayIsoDate()

  return (
    <div className="space-y-2">
      <Label>{t('apiKeys.validity')}</Label>
      <Select value={value.mode} onValueChange={(mode) => onChange({ ...value, mode })}>
        <SelectTrigger className="w-full">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="never">{t('apiKeys.validityNever')}</SelectItem>
          <SelectItem value="7">{t('apiKeys.validity7')}</SelectItem>
          <SelectItem value="30">{t('apiKeys.validity30')}</SelectItem>
          <SelectItem value="90">{t('apiKeys.validity90')}</SelectItem>
          <SelectItem value="custom">{t('apiKeys.validityCustom')}</SelectItem>
        </SelectContent>
      </Select>
      {value.mode === 'custom' ? (
        <Input
          type="date"
          min={today}
          value={value.date}
          onChange={(event) => onChange({ ...value, date: event.target.value })}
        />
      ) : null}
    </div>
  )
}
