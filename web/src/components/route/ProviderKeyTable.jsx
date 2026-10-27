import { useTranslation } from 'react-i18next'
import { PencilIcon, RotateCcwIcon, TrashIcon } from 'lucide-react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useIncidentStats } from '@/hooks/useIncidents'
import { useProviderPresets } from '@/hooks/useProviderKeys'
import { healthState, presetDisplayName, presetOf, unprobeableReason } from '@/lib/providerEndpoint'

export default function ProviderKeyTable({
  rows,
  onEdit,
  onDelete,
  onToggleStatus,
  pendingId,
  healthById = {},
  onRestore,
  canRestore = false,
  readOnly = false,
}) {
  const { t, i18n } = useTranslation()
  const { data: presets = [] } = useProviderPresets()
  const { data: stats = [] } = useIncidentStats()
  const statsById = new Map(stats.map((stat) => [stat.provider_id, stat]))

  if (!rows.length) {
    return <p className="py-8 text-center text-sm text-muted-foreground">{t('providers.empty')}</p>
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>{t('providers.provider')}</TableHead>
          <TableHead>{t('providers.models')}</TableHead>
          <TableHead>{t('providers.apiKey')}</TableHead>
          <TableHead>{t('health.column')}</TableHead>
          <TableHead>{t('incidents.recent')}</TableHead>
          <TableHead>{t('providers.priority')}</TableHead>
          <TableHead>{t('providers.status')}</TableHead>
          <TableHead className="text-right">{t('common.actions')}</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row) => {
          const preset = presetOf(presets, row.provider)
          const label =
            row.name ||
            presetDisplayName(preset, i18n.resolvedLanguage ?? i18n.language) ||
            t('providers.customProviderLabel')
          return (
          <TableRow key={row.id}>
            <TableCell className="max-w-[220px]">
              <div className="font-medium">{label}</div>
              <div className="truncate text-xs font-normal text-muted-foreground">
                {preset ? `${row.provider} · ` : ''}
                {(row.api_formats ?? []).length
                  ? row.api_formats
                      .map((format) =>
                        t(`providers.formats.${format}`, { defaultValue: format }),
                      )
                      .join(' · ')
                  : t('providers.noFormats')}
              </div>
              <div
                className="truncate text-xs font-normal text-muted-foreground"
                title={row.base_url ?? ''}
              >
                {row.base_url || '-'}
              </div>
            </TableCell>
            <TableCell className="max-w-[160px]">
              <div
                className="truncate text-xs text-muted-foreground"
                title={(row.models ?? [])
                  .map((mapping) =>
                    mapping.upstream && mapping.upstream !== mapping.name
                      ? `${mapping.name} → ${mapping.upstream}`
                      : mapping.name,
                  )
                  .join(', ')}
              >
                {row.models?.length
                  ? row.models
                      .map((mapping) =>
                        mapping.upstream && mapping.upstream !== mapping.name
                          ? `${mapping.name} → ${mapping.upstream}`
                          : mapping.name,
                      )
                      .join(', ')
                  : '—'}
              </div>
            </TableCell>
            <TableCell className="font-mono text-xs">{row.credential_masked}</TableCell>
            <TableCell>
              {(() => {
                const state = healthState(row, healthById[row.id])
                if (state === 'retired' || state === 'cooldown') {
                  return (
                    <div className="flex items-center gap-1.5">
                      <Badge variant="destructive" title={healthById[row.id]?.reason}>
                        {state === 'retired' ? t('health.retired') : t('health.cooldown')}
                      </Badge>
                      {canRestore ? (
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          onClick={() => onRestore(row)}
                          title={t('health.restore')}
                          aria-label={t('health.restore')}
                        >
                          <RotateCcwIcon className="size-4" />
                        </Button>
                      ) : null}
                    </div>
                  )
                }
                if (state === 'unprobeable') {
                  return (
                    <Badge variant="outline" title={t(unprobeableReason(row))}>
                      {t('health.unprobeable')}
                    </Badge>
                  )
                }
                return <Badge variant="secondary">{t('health.healthy')}</Badge>
              })()}
            </TableCell>
            <TableCell className="tabular-nums text-muted-foreground">
              {(() => {
                const stat = statsById.get(row.id)
                if (!stat || !stat.count) return '-'
                const minutes = Math.round(stat.duration_secs / 60)
                const duration =
                  minutes < 60 ? `${minutes || 1}m` : `${(minutes / 60).toFixed(1)}h`
                return `${stat.count} · ${duration}`
              })()}
            </TableCell>
            <TableCell className="tabular-nums text-muted-foreground">
              {row.priority ?? 1}
            </TableCell>
            <TableCell>
              <Switch
                checked={row.status === 'active'}
                disabled={readOnly || pendingId === row.id}
                onCheckedChange={(checked) => onToggleStatus?.(row, checked)}
                aria-label={t('providers.status')}
              />
            </TableCell>
            <TableCell className="text-right whitespace-nowrap">
              {readOnly ? (
                <span className="text-xs text-muted-foreground">—</span>
              ) : (
                <>
                  <Button variant="ghost" size="icon-sm" onClick={() => onEdit(row)} aria-label={t('common.edit')}>
                    <PencilIcon className="size-4" />
                  </Button>
                  <Button variant="ghost" size="icon-sm" onClick={() => onDelete(row)} aria-label={t('common.delete')}>
                    <TrashIcon className="size-4" />
                  </Button>
                </>
              )}
            </TableCell>
          </TableRow>
          )
        })}
      </TableBody>
    </Table>
  )
}
