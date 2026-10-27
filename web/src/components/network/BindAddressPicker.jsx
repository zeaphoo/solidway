import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { XIcon } from 'lucide-react'

import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useNetworkAddresses } from '@/hooks/useNetworkAddresses'

const IPV4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/
const IPV6 = /^[0-9a-fA-F:]+$/

/** 常见地址：不依赖后端接口，离线也能选。 */
const PRESETS = [
  { ip: '127.0.0.1', labelKey: 'bindPresetLoopback' },
  { ip: '0.0.0.0', labelKey: 'bindPresetAny' },
  { ip: '::1', labelKey: 'bindPresetLoopback6' },
  { ip: '::', labelKey: 'bindPresetAny6' },
]

function isIp(value) {
  const trimmed = value.trim()
  if (IPV4.test(trimmed)) {
    return trimmed.split('.').every((part) => Number(part) <= 255)
  }
  return trimmed.includes(':') && IPV6.test(trimmed)
}

function isWildcard(value) {
  return value === '0.0.0.0' || value === '::'
}

/**
 * 绑定地址：已选地址以芯片展示，通过「选择常见地址」下拉或手动输入添加。
 *
 * 规则与后端一致：通配地址（0.0.0.0 / ::）互斥，不能与其他地址共存。
 */
export default function BindAddressPicker({ value = [], onChange }) {
  const { t } = useTranslation()
  const { data, isPending, isError } = useNetworkAddresses()
  const [draft, setDraft] = useState('')
  const [picker, setPicker] = useState('')
  const [showVirtual, setShowVirtual] = useState(false)
  const [error, setError] = useState(null)

  const selected = value
  const wildcardSelected = selected.some(isWildcard)
  // 后端返回 [{ ip, interface, virtual_nic }]；虚拟网卡（docker / 网桥 / 隧道）默认收起。
  const allLocal = (data?.local ?? [])
    .map((item) =>
      typeof item === 'string'
        ? { ip: item, interface: '', virtual_nic: false }
        : item,
    )
    .filter((item) => !PRESETS.some((preset) => preset.ip === item.ip))
  // 默认只列「每张物理网卡 × 每个协议族」的第一个地址：临时 IPv6 / 虚拟网卡折叠起来。
  const primaryLocal = []
  const seenFamily = new Set()
  for (const item of allLocal) {
    if (item.virtual_nic) continue
    const family = item.ip.includes(':') ? 6 : 4
    const key = `${item.interface}|${family}`
    if (seenFamily.has(key)) continue
    seenFamily.add(key)
    primaryLocal.push(item)
  }
  const localCandidates = showVirtual ? allLocal : primaryLocal
  const hiddenLocal = allLocal.length - localCandidates.length

  const apply = (ip) => {
    if (isWildcard(ip)) {
      onChange([ip])
      return
    }
    const next = selected.filter((item) => !isWildcard(item))
    if (!next.includes(ip)) next.push(ip)
    onChange(next)
  }

  const addManual = () => {
    const candidate = draft.trim()
    if (!candidate) return
    if (!isIp(candidate)) {
      setError(t('settings.bindAddrInvalid'))
      return
    }
    setError(null)
    setDraft('')
    apply(candidate)
  }

  return (
    <div className="space-y-3">
      <Label>{t('settings.bindAddrs')}</Label>

      <div className="flex flex-wrap gap-2">
        {selected.length ? (
          selected.map((ip) => (
            <span
              key={`selected-${ip}`}
              className="inline-flex items-center gap-1 rounded-full border bg-muted px-2.5 py-1 font-mono text-xs"
            >
              {ip}
              <button
                type="button"
                aria-label={t('settings.bindAddrRemove')}
                onClick={() => onChange(selected.filter((item) => item !== ip))}
              >
                <XIcon className="size-3" />
              </button>
            </span>
          ))
        ) : (
          <span className="text-xs text-muted-foreground">{t('settings.bindAddrEmpty')}</span>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={picker || undefined}
          onValueChange={(next) => {
            setPicker('')
            apply(next)
          }}
        >
          <SelectTrigger className="w-64">
            <SelectValue placeholder={t('settings.bindAddrPick')} />
          </SelectTrigger>
          <SelectContent>
            {PRESETS.map(({ ip, labelKey }) => (
              <SelectItem
                key={ip}
                value={ip}
                disabled={selected.includes(ip) || (wildcardSelected && !isWildcard(ip))}
              >
                <span className="font-mono">{ip}</span>
                <span className="ml-2 text-muted-foreground">{t(`settings.${labelKey}`)}</span>
              </SelectItem>
            ))}
            {localCandidates.map(({ ip, interface: iface, virtual_nic: virtualNic }) => (
              <SelectItem
                key={ip}
                value={ip}
                disabled={selected.includes(ip) || wildcardSelected}
              >
                <span className="font-mono">{ip}</span>
                <span className="ml-2 text-muted-foreground">
                  {iface ? `${t('settings.bindPresetLan')} · ${iface}` : t('settings.bindPresetLan')}
                  {virtualNic ? ` (${t('settings.bindVirtualNic')})` : ''}
                </span>
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <span className="text-xs text-muted-foreground">{t('settings.bindAddrOrManual')}</span>

        <Input
          value={draft}
          placeholder={t('settings.bindAddrPlaceholder')}
          onChange={(event) => {
            setDraft(event.target.value)
            setError(null)
          }}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              addManual()
            }
          }}
          className="max-w-48"
        />
        <button
          type="button"
          onClick={addManual}
          className="rounded-md border px-3 py-1.5 text-xs hover:bg-muted"
        >
          {t('settings.bindAddrAdd')}
        </button>
      </div>

      {hiddenLocal > 0 && !showVirtual ? (
        <button
          type="button"
          className="text-xs text-muted-foreground underline-offset-2 hover:underline"
          onClick={() => setShowVirtual(true)}
        >
          {t('settings.bindShowAllLocal', { count: hiddenLocal })}
        </button>
      ) : null}
      {showVirtual ? (
        <button
          type="button"
          className="text-xs text-muted-foreground underline-offset-2 hover:underline"
          onClick={() => setShowVirtual(false)}
        >
          {t('settings.bindHideAllLocal')}
        </button>
      ) : null}

      {error ? <p className="text-xs text-destructive">{error}</p> : null}
      {wildcardSelected ? (
        <p className="text-xs text-amber-600 dark:text-amber-500">
          {t('settings.bindWildcardHint')}
        </p>
      ) : null}
      {isPending ? (
        <p className="text-xs text-muted-foreground">{t('settings.bindLocalDetecting')}</p>
      ) : isError ? (
        <p className="text-xs text-muted-foreground">{t('settings.bindLocalUnavailable')}</p>
      ) : (data?.local ?? []).length === 0 ? (
        <p className="text-xs text-muted-foreground">{t('settings.bindLocalEmpty')}</p>
      ) : null}

      <p className="text-xs text-muted-foreground">{t('settings.bindAddrsHint')}</p>
    </div>
  )
}
