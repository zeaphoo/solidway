import { CheckIcon, CopyIcon } from 'lucide-react'
import hljs from 'highlight.js/lib/core'
import bash from 'highlight.js/lib/languages/bash'
import ini from 'highlight.js/lib/languages/ini'
import javascript from 'highlight.js/lib/languages/javascript'
import json from 'highlight.js/lib/languages/json'
import python from 'highlight.js/lib/languages/python'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'

import { Button } from '@/components/ui/button'
import {
  ENDPOINTS,
  FORMATS,
  HARNESSES,
  SDK_LANGUAGE,
  SDK_TABS,
  buildHarnessSnippet,
  buildSdkSnippet,
  harnessFormat,
} from '@/lib/accessSnippets'

hljs.registerLanguage('bash', bash)
hljs.registerLanguage('json', json)
hljs.registerLanguage('ini', ini)
hljs.registerLanguage('python', python)
hljs.registerLanguage('javascript', javascript)

const FORMAT_HINT = {
  chat_completions: 'access.formatHintChat',
  responses: 'access.formatHintResponses',
  anthropic_messages: 'access.formatHintAnthropic',
}

/**
 * 调用示例与终端 Agent 接入（内容区）。
 *
 * 路由详情页的对话框与帮助中心共用这一份实现：两级标签（SDK / 终端 Agent）
 * + 底部一个代码区与复制按钮。示例一律由 `lib/accessSnippets` 生成。
 */
export default function AccessGuidePanel({
  baseUrl,
  modelsByFormat = {},
  formats = [],
  showNotes = true,
}) {
  const { t } = useTranslation()
  const [group, setGroup] = useState('sdk')
  const [sdkTab, setSdkTab] = useState('curl')
  const [harnessTab, setHarnessTab] = useState(HARNESSES[0].key)
  const [format, setFormat] = useState('')
  const [copied, setCopied] = useState(false)

  const activeFormat = formats.includes(format) ? format : (formats[0] ?? 'chat_completions')
  const requiredFormat = harnessFormat(harnessTab, formats)
  const model =
    (modelsByFormat[group === 'sdk' ? activeFormat : requiredFormat] ?? [])[0] ?? '<MODEL>'

  const { snippet, language } = useMemo(() => {
    if (group === 'sdk') {
      return {
        snippet: buildSdkSnippet(baseUrl, model, activeFormat, sdkTab),
        language: SDK_LANGUAGE[sdkTab],
      }
    }
    const harness = HARNESSES.find((item) => item.key === harnessTab) ?? HARNESSES[0]
    return {
      snippet: buildHarnessSnippet(harnessTab, baseUrl, model, formats),
      language: harness.language,
    }
  }, [group, sdkTab, harnessTab, baseUrl, model, activeFormat, formats])

  const highlighted = useMemo(
    () => hljs.highlight(snippet, { language }).value,
    [snippet, language],
  )

  const endpoints = FORMATS.filter((item) => formats.includes(item)).map((item) => ENDPOINTS[item])
  const missingFormat = group === 'agent' && formats.length > 0 && !formats.includes(requiredFormat)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(snippet)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1500)
    } catch {
      toast.error(t('common.copyFailed'))
    }
  }

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-1">
        <Button
          type="button"
          variant={group === 'sdk' ? 'secondary' : 'ghost'}
          size="sm"
          onClick={() => setGroup('sdk')}
        >
          {t('access.groupSdk')}
        </Button>
        <Button
          type="button"
          variant={group === 'agent' ? 'secondary' : 'ghost'}
          size="sm"
          onClick={() => setGroup('agent')}
        >
          {t('access.groupAgent')}
        </Button>

        <span className="mx-2 h-4 w-px bg-border" />

        {group === 'sdk'
          ? FORMATS.filter((item) => formats.includes(item)).map((item) => (
              <Button
                key={item}
                type="button"
                variant={activeFormat === item ? 'secondary' : 'ghost'}
                size="xs"
                onClick={() => setFormat(item)}
              >
                {t(`providers.formats.${item}`)}
              </Button>
            ))
          : null}
      </div>

      <div className="flex flex-wrap items-center gap-1">
        {(group === 'sdk' ? SDK_TABS : HARNESSES.map((item) => item.key)).map((name) => (
          <Button
            key={name}
            type="button"
            variant={
              (group === 'sdk' ? sdkTab === name : harnessTab === name) ? 'secondary' : 'ghost'
            }
            size="xs"
            onClick={() => (group === 'sdk' ? setSdkTab(name) : setHarnessTab(name))}
          >
            {group === 'sdk' ? t(`access.tabs.${name}`) : t(`harness.tabs.${name}`)}
          </Button>
        ))}
      </div>

      <p className="text-xs text-muted-foreground">
        {group === 'sdk'
          ? t(FORMAT_HINT[activeFormat] ?? FORMAT_HINT.chat_completions)
          : t(`harness.notes.${harnessTab}`)}
      </p>

      {missingFormat ? (
        <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs">
          {t('harness.missingFormat', { format: requiredFormat })}
        </p>
      ) : null}

      <div className="flex items-center justify-between gap-2">
        <div className="flex flex-wrap gap-1.5">
          {endpoints.map((endpoint) => (
            <code key={endpoint} className="rounded border bg-muted px-1.5 py-0.5 font-mono text-xs">
              {endpoint}
            </code>
          ))}
        </div>
        <Button type="button" variant="ghost" size="sm" onClick={copy}>
          {copied ? <CheckIcon className="size-4" /> : <CopyIcon className="size-4" />}
          {t('common.copy')}
        </Button>
      </div>

      <pre className="overflow-x-auto rounded-lg border bg-muted p-3 text-xs leading-relaxed">
        <code dangerouslySetInnerHTML={{ __html: highlighted }} />
      </pre>

      {showNotes ? (
        <ul className="space-y-1 text-xs text-muted-foreground">
          <li>{t('access.snippetHint')}</li>
          <li>{t('harness.keyHint')}</li>
          <li>{t('harness.modelHint')}</li>
        </ul>
      ) : null}
    </div>
  )
}
