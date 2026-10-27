import { useTranslation } from 'react-i18next'

import { Skeleton } from '@/components/ui/skeleton'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { formatMs, formatTokens } from '@/lib/usage'

export default function UsageBreakdownTable({ rows, by, isPending, showOwner = false }) {
  const { t } = useTranslation()

  if (isPending) return <Skeleton className="h-32 w-full" />
  if (!rows?.length) {
    return <p className="py-8 text-center text-sm text-muted-foreground">{t('usage.empty')}</p>
  }

  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t('usage.dimension')}</TableHead>
            {showOwner && by !== 'owner' ? <TableHead>{t('usage.owner')}</TableHead> : null}
            <TableHead>{t('usage.requests')}</TableHead>
            <TableHead>{t('usage.errors')}</TableHead>
            <TableHead>{t('usage.inputTokens')}</TableHead>
            <TableHead>{t('usage.outputTokens')}</TableHead>
            <TableHead>{t('usage.cachedTokens')}</TableHead>
            <TableHead>{t('usage.avgLatency')}</TableHead>
            <TableHead>{t('usage.p95')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={`${by}-${row.key || 'empty'}-${row.route_id || ''}`}>
              <TableCell className="font-medium">
                {row.label || row.key || '-'}
                {row.archived ? (
                  <span className="ml-1 text-xs text-muted-foreground">
                    {t('common.archivedSuffix')}
                  </span>
                ) : null}
                {row.deleted ? (
                  <span className="ml-1 text-xs text-muted-foreground">
                    {t('common.deletedSuffix')}
                  </span>
                ) : null}
              </TableCell>
              {showOwner && by !== 'owner' ? (
                <TableCell className="text-muted-foreground">{row.owner_name || '-'}</TableCell>
              ) : null}
              <TableCell className="tabular-nums">{row.requests}</TableCell>
              <TableCell className="tabular-nums">{row.errors}</TableCell>
              <TableCell className="tabular-nums">{formatTokens(row.input_tokens)}</TableCell>
              <TableCell className="tabular-nums">{formatTokens(row.output_tokens)}</TableCell>
              <TableCell className="tabular-nums">{formatTokens(row.cached_tokens)}</TableCell>
              <TableCell className="tabular-nums">{formatMs(row.latency_avg_ms)}</TableCell>
              <TableCell className="tabular-nums">{formatMs(row.p95_ms)}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
