import { Fragment, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { formatUnix } from '@/lib/format'
import { formatTokens } from '@/lib/usage'

/**
 * 请求明细表：点击行展开每次尝试（provider / 状态 / 延迟 / 错误）。
 *
 * `showOwner` 供全局统计页展示归属用户；路由名与 Key 名优先用服务端返回的标签。
 */
export default function UsageRequestTable({ rows, showOwner = false, routeName, apiKeyLabel }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(null)

  const toggle = (id) => setExpanded((current) => (current === id ? null : id))

  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t('usage.time')}</TableHead>
            {showOwner ? <TableHead>{t('usage.owner')}</TableHead> : null}
            <TableHead>{t('usage.byRoute')}</TableHead>
            <TableHead>{t('usage.byApiKey')}</TableHead>
            <TableHead>{t('usage.model')}</TableHead>
            <TableHead>{t('usage.provider')}</TableHead>
            <TableHead>{t('providers.status')}</TableHead>
            <TableHead>{t('usage.attempts')}</TableHead>
            <TableHead>{t('usage.latency')}</TableHead>
            <TableHead>{t('usage.inputTokens')}</TableHead>
            <TableHead>{t('usage.outputTokens')}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => {
            const open = expanded === row.request_id
            const attempts = row.attempts_detail ?? []
            // R4-W-3：map 根节点用带 key 的 Fragment，展开行不再各自造 key。
            return (
              <Fragment key={row.request_id}>
                <TableRow
                  className="cursor-pointer"
                  onClick={() => toggle(row.request_id)}
                >
                  <TableCell className="text-muted-foreground tabular-nums">
                    {formatUnix(row.at)}
                  </TableCell>
                  {showOwner ? <TableCell>{row.owner_name || '-'}</TableCell> : null}
                  <TableCell className="font-medium">
                    {row.route_name || routeName?.(row.route_id) || row.route_id}
                    {row.route_archived ? (
                      <span className="ml-1 text-xs text-muted-foreground">
                        {t('common.archivedSuffix')}
                      </span>
                    ) : null}
                    {row.route_deleted ? (
                      <span className="ml-1 text-xs text-muted-foreground">
                        {t('common.deletedSuffix')}
                      </span>
                    ) : null}
                  </TableCell>
                  <TableCell>
                    {row.api_key_label || apiKeyLabel?.(row) || row.api_key_id}
                    {row.api_key_deleted ? (
                      <span className="ml-1 text-xs text-muted-foreground">
                        {t('common.deletedSuffix')}
                      </span>
                    ) : null}
                  </TableCell>
                  <TableCell>{row.model || '-'}</TableCell>
                  <TableCell>
                    {row.provider_name || row.provider_id || '-'}
                    {row.provider_deleted ? (
                      <span className="ml-1 text-xs text-muted-foreground">
                        {t('common.deletedSuffix')}
                      </span>
                    ) : null}
                  </TableCell>
                  <TableCell className="tabular-nums">{row.status}</TableCell>
                  <TableCell className="tabular-nums">
                    <span className="inline-flex items-center gap-1">
                      {row.attempts}
                      {attempts.length ? (
                        <span className="text-xs text-muted-foreground">
                          {open ? '▾' : '▸'}
                        </span>
                      ) : null}
                    </span>
                  </TableCell>
                  <TableCell className="tabular-nums">{row.latency_ms} ms</TableCell>
                  <TableCell className="tabular-nums">
                    {formatTokens(row.input_tokens)}
                  </TableCell>
                  <TableCell className="tabular-nums">
                    {formatTokens(row.output_tokens)}
                  </TableCell>
                </TableRow>
                {open ? (
                  <TableRow>
                    <TableCell colSpan={showOwner ? 11 : 10} className="bg-muted/40">
                      {attempts.length ? (
                        <div className="space-y-1 py-1">
                          <div className="text-xs text-muted-foreground">
                            {t('usage.attemptsDetail')}
                          </div>
                          {attempts.map((attempt, index) => (
                            <div
                              key={`${row.request_id}-${index}`}
                              className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs"
                            >
                              <Badge variant="secondary" className="tabular-nums">
                                #{index + 1}
                              </Badge>
                              <code className="font-mono">{attempt.provider_id || '-'}</code>
                              <span className="tabular-nums text-muted-foreground">
                                {attempt.status ?? '-'}
                              </span>
                              <span className="tabular-nums text-muted-foreground">
                                {attempt.latency_ms} ms
                              </span>
                              {attempt.error ? (
                                <span className="text-destructive">{attempt.error}</span>
                              ) : null}
                            </div>
                          ))}
                        </div>
                      ) : (
                        <p className="py-1 text-xs text-muted-foreground">
                          {t('usage.attemptsEmpty')}
                        </p>
                      )}
                    </TableCell>
                  </TableRow>
                ) : null}
              </Fragment>
            )
          })}
        </TableBody>
      </Table>
    </div>
  )
}
