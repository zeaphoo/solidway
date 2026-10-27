import { ArrowRightIcon, ChevronDownIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router'

import DiagnosticsCard from '@/components/help/DiagnosticsCard'
import SnippetSection from '@/components/help/SnippetSection'
import { cn } from '@/lib/utils'

/** 正文里的反引号片段渲染成等宽 `<code>`，避免引 markdown 解析器。 */
function TextWithCode({ text }) {
  const parts = String(text ?? '').split('`')

  return (
    <p className="text-sm leading-relaxed text-muted-foreground">
      {parts.map((part, index) =>
        index % 2 === 1 ? (
          <code
            key={index}
            className="rounded border bg-muted px-1 py-0.5 font-mono text-[11px] text-foreground"
          >
            {part}
          </code>
        ) : (
          part
        ),
      )}
    </p>
  )
}

function Item({ item }) {
  const { t } = useTranslation()
  const rawPoints = t(`help.items.${item.id}.points`, { returnObjects: true })
  const points = Array.isArray(rawPoints) ? rawPoints : []

  return (
    <article id={item.id} className="scroll-mt-4 space-y-2 rounded-lg border p-4">
      <h3 className="text-sm font-medium">{t(`help.items.${item.id}.title`)}</h3>
      <TextWithCode text={t(`help.items.${item.id}.body`)} />
      {points.length ? (
        <ul className="list-disc space-y-1 pl-5 text-sm text-muted-foreground">
          {points.map((point, index) => (
            <li key={index}>{point}</li>
          ))}
        </ul>
      ) : null}
      {item.to ? (
        <Link
          to={item.to}
          className="inline-flex items-center gap-1 text-xs text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
        >
          {t('help.open')}
          <ArrowRightIcon className="size-3" />
        </Link>
      ) : null}
    </article>
  )
}

/** 帮助分区：标题可折叠；锚点直达时只展开目标分区。 */
export default function HelpSection({ section, items, expanded, expandedByQuery, onToggle }) {
  const { t } = useTranslation()
  const open = expandedByQuery || expanded

  return (
    <section id={section.id} className="scroll-mt-4 border-b pb-6 last:border-0">
      <h2>
        <button
          type="button"
          onClick={onToggle}
          aria-expanded={open}
          className="flex w-full items-center justify-between gap-3 py-1 text-left"
        >
          <span className="text-base font-semibold">{t(section.titleKey)}</span>
          <ChevronDownIcon
            className={cn(
              'size-4 shrink-0 text-muted-foreground transition-transform',
              open && 'rotate-180',
            )}
          />
        </button>
      </h2>

      {open ? (
        <div className="space-y-3 pt-2">
          <p className="text-sm text-muted-foreground">{t(section.descKey)}</p>
          {section.kind === 'snippets' ? (
            <SnippetSection />
          ) : section.kind === 'diagnostics' ? (
            <DiagnosticsCard />
          ) : (
            <div className="space-y-3">
              {items.map((item) => (
                <Item key={item.id} item={item} />
              ))}
            </div>
          )}
        </div>
      ) : null}
    </section>
  )
}
