import {
  ActivityIcon,
  ArrowRightIcon,
  BookOpenIcon,
  CodeIcon,
  HelpCircleIcon,
  ListChecksIcon,
  SearchIcon,
  WrenchIcon,
} from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useLocation } from 'react-router'

import HelpSection from '@/components/help/HelpSection'
import { Input } from '@/components/ui/input'
import { HELP_SECTIONS, filterSections, resolveAnchor } from '@/lib/helpContent'
import { cn } from '@/lib/utils'

const ICONS = {
  book: BookOpenIcon,
  list: ListChecksIcon,
  code: CodeIcon,
  wrench: WrenchIcon,
  activity: ActivityIcon,
  help: HelpCircleIcon,
}

export default function HelpPage() {
  const { t } = useTranslation()
  const { hash } = useLocation()
  const anchor = hash.replace(/^#/, '')
  const [query, setQuery] = useState('')
  // 展开态默认由锚点推导（无锚点全展开、有锚点只展开目标分区），
  // 用户手动折叠/展开记为覆盖，避免在 effect 里同步状态。
  const [overrides, setOverrides] = useState({})

  const searching = query.trim().length > 0
  const results = useMemo(() => filterSections(HELP_SECTIONS, query, t), [query, t])
  const anchorSectionId = useMemo(() => resolveAnchor(anchor)?.section.id ?? null, [anchor])
  const isExpanded = (id) => overrides[id] ?? (anchorSectionId ? id === anchorSectionId : true)

  // 锚点变化（含从其他页面跳入）：把目标滚进视野。
  useEffect(() => {
    if (!anchorSectionId) return
    const timer = window.setTimeout(() => {
      document.getElementById(anchor)?.scrollIntoView({ block: 'start' })
    }, 0)
    return () => window.clearTimeout(timer)
  }, [anchor, anchorSectionId])

  const toggle = (id) => setOverrides((prev) => ({ ...prev, [id]: !isExpanded(id) }))

  const toc = (
    <ul className="space-y-0.5">
      {HELP_SECTIONS.map((section) => {
        const Icon = ICONS[section.icon] ?? HelpCircleIcon
        return (
          <li key={section.id}>
            <Link
              to={`/help#${section.id}`}
              className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
            >
              <Icon className="size-4 shrink-0" />
              <span className="truncate">{t(section.titleKey)}</span>
            </Link>
          </li>
        )
      })}
    </ul>
  )

  return (
    <div className="mx-auto flex max-w-5xl gap-8">
      <aside className="hidden w-44 shrink-0 lg:block">
        <div className="sticky top-0 space-y-2">
          <p className="px-2 text-[11px] font-semibold tracking-wider text-muted-foreground uppercase">
            {t('help.toc')}
          </p>
          {toc}
          <div className="border-t pt-2">
            <Link
              to="/welcome"
              className="flex items-center gap-1 rounded-md px-2 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
            >
              {t('help.goOnboarding')}
              <ArrowRightIcon className="size-3" />
            </Link>
          </div>
        </div>
      </aside>

      <div className="min-w-0 flex-1 space-y-6">
        <div className="space-y-3">
          <div className="space-y-1">
            <h1 className="text-xl font-semibold">{t('help.title')}</h1>
            <p className="text-sm text-muted-foreground">{t('help.subtitle')}</p>
          </div>

          <div className="relative max-w-md">
            <SearchIcon className="absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t('help.searchPlaceholder')}
              aria-label={t('help.searchPlaceholder')}
              className="pl-8"
            />
          </div>

          {/* 窄屏目录：sticky 侧栏放不下，退化为横向锚点。 */}
          <div className="flex flex-wrap gap-1.5 lg:hidden">
            {HELP_SECTIONS.map((section) => (
              <Link
                key={section.id}
                to={`/help#${section.id}`}
                className={cn(
                  'rounded-full border px-2.5 py-1 text-xs text-muted-foreground',
                  'transition-colors hover:text-foreground',
                )}
              >
                {t(section.titleKey)}
              </Link>
            ))}
          </div>
        </div>

        {results.length ? (
          <div className="space-y-6">
            {results.map(({ section, items }) => (
              <HelpSection
                key={section.id}
                section={section}
                items={items}
                expanded={isExpanded(section.id)}
                expandedByQuery={searching}
                onToggle={() => toggle(section.id)}
              />
            ))}
          </div>
        ) : (
          <p className="py-16 text-center text-sm text-muted-foreground">{t('help.searchEmpty')}</p>
        )}
      </div>
    </div>
  )
}
