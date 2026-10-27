import { CheckIcon, ChevronsUpDownIcon, SearchIcon } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from 'cn'
import { presetDisplayName } from '@/lib/providerEndpoint'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'

/**
 * 可搜索的 Provider 预设选择器。
 *
 * 与普通 Select 不同：目录有 200+ 条，选择器始终展示全部预设，并按
 * “常用 / 全部”分组；输入关键字时在全部预设中搜索。
 */
export default function ProviderPresetCombobox({
  presets,
  value,
  onChange,
  customValue,
  placeholder,
  customLabel,
  featuredLabel,
  othersLabel,
  searchPlaceholder,
  emptyLabel,
}) {
  const { t, i18n } = useTranslation()
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(0)
  const optionRefs = useRef([])

  const selected = presets.find((preset) => preset.key === value)
  const isCustom = value === customValue || (!selected && Boolean(value))

  const variantLabel = (preset) =>
    preset.variant_label ? t(preset.variant_label, { defaultValue: '' }) : ''

  const presetName = (preset) =>
    presetDisplayName(preset, i18n.resolvedLanguage ?? i18n.language)

  const kindLabel = (preset) =>
    t(`providers.presetKinds.${preset.kind ?? 'api'}.label`, {
      defaultValue: preset.kind ?? 'api',
    })

  const presetSummary = (preset) => {
    if (preset.summary) {
      return t(preset.summary, { defaultValue: '' })
    }
    return t(`providers.presetKinds.${preset.kind ?? 'api'}.hint`, {
      name: presetName(preset),
      defaultValue: '',
    })
  }

  const filtered = useMemo(() => {
    const keyword = query.trim().toLowerCase()
    const matched = keyword
      ? presets.filter(
          (preset) =>
            preset.name.toLowerCase().includes(keyword) ||
            (preset.name_zh ?? '').toLowerCase().includes(keyword) ||
            preset.key.toLowerCase().includes(keyword) ||
            variantLabel(preset).toLowerCase().includes(keyword) ||
            presetSummary(preset).toLowerCase().includes(keyword),
        )
      : presets
    // 同一预设族（group 相同）相邻展示，族内保持目录顺序。
    const rank = new Map()
    return [...matched].sort((a, b) => {
      const groupA = a.group ? `g:${a.group}` : `k:${a.key}`
      const groupB = b.group ? `g:${b.group}` : `k:${b.key}`
      if (!rank.has(groupA)) rank.set(groupA, rank.size)
      if (!rank.has(groupB)) rank.set(groupB, rank.size)
      return rank.get(groupA) - rank.get(groupB)
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [presets, query, t, i18n.language])

  const options = useMemo(
    () => [
      ...filtered,
      {
        key: customValue,
        name: customLabel,
        custom: true,
      },
    ],
    [filtered, customValue, customLabel],
  )

  const featured = filtered.filter((preset) => preset.featured)
  const others = filtered.filter((preset) => !preset.featured)
  const showGroups = !query.trim() && (featured.length > 0 || others.length > 0)

  useEffect(() => {
    // 自定义项固定在滚动区外，不需要也不应该触发页面滚动。
    if (activeIndex < options.length - 1) {
      optionRefs.current[activeIndex]?.scrollIntoView({ block: 'nearest' })
    }
  }, [activeIndex, options.length])

  const select = (next) => {
    onChange(next)
    setOpen(false)
  }

  const onKeyDown = (event) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setActiveIndex((index) => Math.min(index + 1, options.length - 1))
      return
    }
    if (event.key === 'ArrowUp') {
      event.preventDefault()
      setActiveIndex((index) => Math.max(index - 1, 0))
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      const option = options[activeIndex]
      if (option) select(option.key)
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      setOpen(false)
    }
  }

  const renderOption = (option, index) => {
    const active = index === activeIndex
    const checked = option.key === value
    return (
      <button
        key={option.key}
        ref={(node) => {
          optionRefs.current[index] = node
        }}
        type="button"
        role="option"
        aria-selected={checked}
        className={cn(
          'flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm outline-none',
          active && 'bg-accent text-accent-foreground',
        )}
        onMouseEnter={() => setActiveIndex(index)}
        onClick={() => select(option.key)}
      >
        <span className="min-w-0 flex-1">
          {option.custom ? (
            <span className="block truncate">{option.name}</span>
          ) : (
            <>
              <span className="flex items-center gap-2">
                <span className="truncate">{presetName(option)}</span>
                {option.variant_label ? (
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {t(option.variant_label, { defaultValue: '' })}
                  </span>
                ) : null}
                <span className="shrink-0 rounded border px-1.5 py-0.5 text-[10px] text-muted-foreground">
                  {kindLabel(option)}
                </span>
              </span>
              <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                {presetSummary(option)}
              </span>
              <span className="mt-0.5 block font-mono text-[10px] text-muted-foreground">
                {option.key}
              </span>
            </>
          )}
        </span>
        {checked ? <CheckIcon className="size-4 shrink-0" /> : null}
      </button>
    )
  }

  const optionIndex = (option) => options.findIndex((item) => item.key === option.key)

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (next) {
          setQuery('')
          const selectedIndex = presets.findIndex((preset) => preset.key === value)
          setActiveIndex(selectedIndex >= 0 ? selectedIndex : 0)
        }
      }}
    >
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="outline"
          role="combobox"
          aria-haspopup="listbox"
          aria-expanded={open}
          className="w-full justify-between font-normal"
        >
          <span className="min-w-0 truncate">
            {isCustom
              ? customLabel
              : selected
                ? `${presetName(selected)} · ${selected.key}`
                : placeholder}
          </span>
          <ChevronsUpDownIcon className="size-4 shrink-0 opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        className="w-[var(--radix-popover-trigger-width)] p-0"
      >
        <div className="flex items-center border-b px-2">
          <SearchIcon className="size-4 shrink-0 text-muted-foreground" />
          <Input
            autoFocus
            value={query}
            onChange={(event) => {
              setQuery(event.target.value)
              setActiveIndex(0)
            }}
            onKeyDown={onKeyDown}
            placeholder={searchPlaceholder}
            className="h-9 border-0 shadow-none focus-visible:ring-0"
            aria-label={searchPlaceholder}
          />
        </div>
        <div role="listbox">
          <div className="max-h-72 overflow-y-auto p-1">
            {showGroups ? (
              <>
                {featured.length ? (
                  <>
                    <div className="px-2 py-1 text-xs text-muted-foreground">{featuredLabel}</div>
                    {featured.map((preset) => renderOption(preset, optionIndex(preset)))}
                  </>
                ) : null}
                {others.length ? (
                  <>
                    <div className="mt-1 px-2 py-1 text-xs text-muted-foreground">{othersLabel}</div>
                    {others.map((preset) => renderOption(preset, optionIndex(preset)))}
                  </>
                ) : null}
              </>
            ) : filtered.length ? (
              filtered.map((preset) => renderOption(preset, optionIndex(preset)))
            ) : (
              <p className="px-2 py-6 text-center text-sm text-muted-foreground">{emptyLabel}</p>
            )}
          </div>
          <div className="border-t bg-popover p-1">
            {renderOption(options[options.length - 1], options.length - 1)}
          </div>
        </div>
      </PopoverContent>
    </Popover>
  )
}
