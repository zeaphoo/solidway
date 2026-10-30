import { zodResolver } from '@hookform/resolvers/zod'
import { CopyIcon, PencilIcon, PlusIcon, TrashIcon, Undo2Icon } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Controller, useFieldArray, useForm, useWatch } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { z } from 'zod'

import ProviderPresetCombobox from '@/components/route/ProviderPresetCombobox'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import {
  useCreateProviderKey,
  useProviderPresets,
  useUpdateProviderKey,
} from '@/hooks/useProviderKeys'
import { useDiscoverModels } from '@/hooks/useDiscoverModels'
import { codeMessage, errorMessage } from '@/lib/format'
import {
  detectCredentialKind,
  expandTemplate,
  isPresetEntry,
  presetDisplayName,
  presetOf,
  requiredAuthScheme,
} from '@/lib/providerEndpoint'

const schema = z.object({
  provider: z.string().min(1),
  name: z.string().optional(),
  base_url: z.string().optional(),
  api_formats: z.array(z.string()),
  auth_scheme: z.string().min(1),
  credential: z.string().optional(),
  models: z.array(
    z.object({ name: z.string(), upstream: z.string(), renamed: z.boolean().optional() }),
  ),
  priority: z.coerce.number().int().min(1).max(1000),
  status: z.enum(['active', 'inactive']),
})

const EMPTY = {
  provider: '',
  name: '',
  base_url: '',
  api_formats: [],
  auth_scheme: 'bearer',
  credential: '',
  models: [],
  priority: 1,
  status: 'active',
}

/** 三种可入站 API 格式。 */
const VISIBLE_FORMATS = ['chat_completions', 'responses', 'anthropic_messages']

/** 自定义条目可选的鉴权方式；`google_service_account` 只由凭据内容决定。 */
const API_KEY_SCHEMES = ['bearer', 'x-api-key', 'api-key', 'none']

/** 可被控制台选中的预设鉴权方式（含需要专用凭据的两种）。 */
const SELECTABLE_SCHEMES = [...API_KEY_SCHEMES, 'google_service_account', 'sigv4']

const CUSTOM_PROVIDER = '__custom__'

export default function ProviderKeyFormDialog({ open, onOpenChange, routeId, providerKey }) {
  const { t, i18n } = useTranslation()
  const isEdit = Boolean(providerKey)
  const { data: presets = [] } = useProviderPresets()
  const createMutation = useCreateProviderKey(routeId)
  const updateMutation = useUpdateProviderKey(routeId)
  const [serverError, setServerError] = useState(null)
  const [customMode, setCustomMode] = useState(false)
  const [originPresetKey, setOriginPresetKey] = useState(null)
  const [vars, setVars] = useState({})
  const [pathVars, setPathVars] = useState({})
  const [confirmConvert, setConfirmConvert] = useState(false)
  const credentialRef = useRef(null)

  const selectablePresets = useMemo(
    () => presets.filter((preset) => SELECTABLE_SCHEMES.includes(preset.auth_scheme)),
    [presets],
  )

  const { register, handleSubmit, reset, control, setValue, getValues } = useForm({
    resolver: zodResolver(schema),
    defaultValues: EMPTY,
  })

  const discover = useDiscoverModels()
  const {
    fields: modelFields,
    append: appendModel,
    replace: replaceModels,
    remove: removeModel,
  } = useFieldArray({
    control,
    name: 'models',
  })
  const modelsValue = useWatch({ control, name: 'models' })
  // 默认只显示一个模型 id；需要对外改名时才展开「上游 id + 对外名称」两列。
  const renameModel = (index) => {
    const currentName = (getValues(`models.${index}.name`) ?? '').trim()
    if (!(getValues(`models.${index}.upstream`) ?? '').trim()) {
      setValue(`models.${index}.upstream`, currentName)
    }
    setValue(`models.${index}.renamed`, true)
  }
  const unrenameModel = (index) => {
    const upstream = (getValues(`models.${index}.upstream`) ?? '').trim()
    const currentName = (getValues(`models.${index}.name`) ?? '').trim()
    setValue(`models.${index}.name`, upstream || currentName)
    setValue(`models.${index}.upstream`, '')
    setValue(`models.${index}.renamed`, false)
  }

  useEffect(() => {
    if (!open) return
    setServerError(null)
    setCustomMode(false)
    setConfirmConvert(false)
    const isPreset = isPresetEntry(presets, providerKey?.provider)
    setOriginPresetKey(isPreset ? providerKey?.provider : null)
    setVars(providerKey?.options?.base_url_vars ?? {})
    setPathVars(providerKey?.options?.path_params ?? {})
    reset({
      provider: providerKey?.provider ?? '',
      name: providerKey?.name ?? '',
      base_url: providerKey?.base_url ?? '',
      api_formats: providerKey?.api_formats ?? [],
      auth_scheme:
        providerKey?.auth_scheme && providerKey.auth_scheme !== 'google_service_account'
          ? providerKey.auth_scheme
          : 'bearer',
      credential: '',
      models: (providerKey?.models ?? []).map((mapping) => ({
        ...mapping,
        renamed: (mapping.upstream ?? '').trim() !== (mapping.name ?? '').trim(),
      })),
      priority: providerKey?.priority ?? 1,
      status: providerKey?.status ?? 'active',
    })
  }, [open, providerKey, presets, reset])

  const providerValue = useWatch({ control, name: 'provider' })
  const credentialValue = useWatch({ control, name: 'credential' })
  const baseUrlValue = useWatch({ control, name: 'base_url' })
  const apiFormatsValue = useWatch({ control, name: 'api_formats' })
  const authSchemeValue = useWatch({ control, name: 'auth_scheme' })

  const presetForCurrent = presets.find((preset) => preset.key === providerValue)
  const isPresetMode = Boolean(presetForCurrent) && !customMode
  const presetForOptions = presetForCurrent ?? presetOf(presets, originPresetKey)
  const presetName = (preset) =>
    presetDisplayName(preset, i18n.resolvedLanguage ?? i18n.language)
  const presetKindLabel = (preset) =>
    t(`providers.presetKinds.${preset.kind ?? 'api'}.label`, {
      defaultValue: preset.kind ?? 'api',
    })
  const presetSummary = (preset) =>
    preset.summary
      ? t(preset.summary, { defaultValue: '' })
      : t(`providers.presetKinds.${preset.kind ?? 'api'}.hint`, {
          name: presetName(preset),
          defaultValue: '',
        })

  // 编辑态凭据留空 = 沿用已保存凭据；此时以已存类型为准。
  const credentialKind = credentialValue?.trim()
    ? detectCredentialKind(credentialValue)
    : providerKey?.credential_kind ?? 'api_key'
  const serviceAccount = credentialKind === 'google_service_account'
  const awsCredentials = credentialKind === 'aws_sigv4'
  const forcedAuth = requiredAuthScheme(credentialKind)
  const credentialUnsupported = serviceAccount || awsCredentials

  const urlVarDecls = isPresetMode ? (presetForCurrent.base_url_variables ?? []) : []
  const pathVarDecls = (isPresetMode ? presetForCurrent : presetForOptions)?.path_variables ?? []
  // 同名参数（如 location 同时出现在主机名与项目路径）只渲染一次，提交时两边都带上。
  const varDecls = [
    ...urlVarDecls,
    ...pathVarDecls.filter(
      (variable) => !urlVarDecls.some((decl) => decl.name === variable.name),
    ),
  ]
  const urlVarNames = new Set(urlVarDecls.map((variable) => variable.name))
  const pathVarNames = new Set(pathVarDecls.map((variable) => variable.name))
  const varValue = (name) => vars[name] ?? pathVars[name] ?? ''

  const expandedBaseUrl = isPresetMode
    ? expandTemplate(presetForCurrent.base_url_template, vars)
    : baseUrlValue
  const missingVars = isPresetMode
    ? varDecls.filter((variable) => variable.required && !String(varValue(variable.name)).trim())
    : []

  const applyPreset = (key) => {
    const preset = presets.find((item) => item.key === key)
    if (!preset) return
    setOriginPresetKey(key)
    setVars({})
    setCustomMode(false)
    setValue('auth_scheme', preset.auth_scheme === 'google_service_account' ? 'bearer' : preset.auth_scheme)
    setValue(
      'api_formats',
      (preset.api_formats ?? []).filter((format) => VISIBLE_FORMATS.includes(format)),
    )
    setValue('base_url', '')
  }

  const copyAsCustom = () => {
    const preset = presetForCurrent ?? presetForOptions
    setCustomMode(true)
    setValue('provider', preset ? `${preset.key}-custom` : getValues('provider'))
    setValue('base_url', expandedBaseUrl)
    setValue(
      'api_formats',
      (preset?.api_formats ?? []).filter((format) => VISIBLE_FORMATS.includes(format)),
    )
    setConfirmConvert(false)
    requestAnimationFrame(() => credentialRef.current?.focus())
  }

  const fetchModels = () => {
    const baseUrl = String(expandedBaseUrl ?? '').trim()
    if (!baseUrl) {
      toast.error(t('providers.fetchNeedsBaseUrl'))
      return
    }
    if (credentialUnsupported) {
      toast.error(t('providers.credentialUnsupported'))
      return
    }
    const requestKey = credentialValue?.trim() ?? ''
    if (!requestKey && !isEdit) {
      toast.error(t('providers.fetchNeedsKey'))
      return
    }
    discover.mutate(
      {
        provider: getValues('provider'),
        base_url: baseUrl,
        api_key: requestKey,
        auth_scheme: isPresetMode ? presetForCurrent.auth_scheme : authSchemeValue,
        api_formats: isPresetMode ? presetForCurrent.api_formats : apiFormatsValue ?? [],
        ...(isEdit && !requestKey ? { provider_key_id: providerKey.id } : {}),
      },
      {
        onSuccess: (result) => {
          if (result.models?.length) {
            // 以上游模型 id 为身份合并：保留已改的对外名称与手动新增项，
            // 顺序以上游返回列表为准（手动新增项排在最后）。
            const identity = (row) => (row?.upstream?.trim() || row?.name?.trim() || '')
            const current = getValues('models') ?? []
            const known = new Map()
            for (const row of current) {
              const key = identity(row)
              if (key && !known.has(key)) known.set(key, row)
            }
            const merged = []
            const seen = new Set()
            for (const raw of result.models) {
              const key = String(raw).trim()
              if (!key || seen.has(key)) continue
              seen.add(key)
              const row = known.get(key)
              merged.push(
                row
                  ? {
                      ...row,
                      name: (row.name ?? '').trim() || key,
                      upstream: row.upstream ?? '',
                      renamed: Boolean(row.renamed),
                    }
                  : { name: key, upstream: '', renamed: false },
              )
            }
            for (const row of current) {
              const key = identity(row)
              if (key && seen.has(key)) continue
              if (key) seen.add(key)
              merged.push(row)
            }
            replaceModels(merged)
            toast.success(t('providers.fetchOk', { count: result.models.length }))
          } else {
            toast.error(
              codeMessage(t, result.error_code, null) ??
                t('providers.fetchEmpty', { msg: result.error || '-' }),
            )
          }
        },
        onError: (error) => toast.error(errorMessage(t, error)),
      },
    )
  }

  const mutation = isEdit ? updateMutation : createMutation

  const onSubmit = (values) => {
    if (isPresetMode && missingVars.length) {
      setServerError(t('providers.varsRequired', { name: missingVars[0].name }))
      return
    }
    if (!isPresetMode && !values.provider.trim()) {
      setServerError(t('providers.providerKeyRequired'))
      return
    }
    const options = { ...(providerKey?.options ?? {}) }
    for (const key of ['project', 'location', 'path_template']) {
      const value = values[key]?.trim()
      if (value) options[key] = value
      else delete options[key]
    }
    delete options.base_url_vars

    const body = {
      provider: values.provider.trim(),
      name: values.name || undefined,
      models: (values.models ?? [])
        .map((mapping) => ({
          name: mapping.name.trim(),
          upstream: (mapping.upstream ?? '').trim(),
        }))
        .filter((mapping) => mapping.name.length > 0),
      priority: values.priority,
      status: values.status,
      options: Object.keys(options).length ? options : undefined,
    }
    const pickVars = (names) =>
      Object.fromEntries(
        Object.entries(vars).filter(([name, value]) => names.has(name) && String(value).trim()),
      )
    const urlVars = pickVars(urlVarNames)
    const pathVarsToSend = pickVars(pathVarNames)
    if (Object.keys(urlVars).length) body.base_url_vars = urlVars
    if (Object.keys(pathVarsToSend).length) body.path_vars = pathVarsToSend
    if (!isPresetMode) {
      body.base_url = values.base_url || undefined
      body.api_formats = values.api_formats ?? []
      body.auth_scheme = forcedAuth ?? values.auth_scheme
    }
    const raw = values.credential?.trim()
    if (raw) {
      if (detectCredentialKind(raw) === 'google_service_account') body.service_account_json = raw
      else body.api_key = raw
    } else if (!isEdit) {
      setServerError(t('providers.credentialRequired'))
      return
    }

    mutation.mutate(isEdit ? { id: providerKey.id, body } : body, {
      onSuccess: (created) => {
        toast.success(isEdit ? t('providers.updated') : t('providers.created'))
        if (created?.autodiscover?.status === 'failed') {
          toast.error(
            codeMessage(t, created.autodiscover.error_code, t('providers.autodiscoverFailed')),
          )
        }
        onOpenChange(false)
      },
      onError: (error) => toast.error(errorMessage(t, error)),
    })
  }

  const presetLocked = isPresetMode

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[90svh] flex-col p-5 sm:max-w-2xl sm:p-6 lg:max-w-3xl">
        <form onSubmit={handleSubmit(onSubmit)} className="flex min-h-0 flex-1 flex-col gap-5">
          <DialogHeader>
            <DialogTitle>{isEdit ? t('providers.edit') : t('providers.new')}</DialogTitle>
            <DialogDescription>{t('providers.maskedHint')}</DialogDescription>
          </DialogHeader>

          <div className="-mx-5 min-h-0 flex-1 overflow-y-auto sm:-mx-6">
            <div className="space-y-4 px-5 py-1 sm:px-6">
              <div className="space-y-2">
                <Label>{t('providers.preset')}</Label>
                <Controller
                  control={control}
                  name="provider"
                  render={({ field }) => (
                    <ProviderPresetCombobox
                      presets={selectablePresets}
                      value={isPresetMode ? field.value : CUSTOM_PROVIDER}
                      customValue={CUSTOM_PROVIDER}
                      placeholder={t('providers.selectProvider')}
                      customLabel={t('providers.customPreset')}
                      featuredLabel={t('providers.featuredGroup')}
                      othersLabel={t('providers.otherGroup')}
                      searchPlaceholder={t('providers.searchProvider')}
                      emptyLabel={t('providers.searchEmpty')}
                      onChange={(value) => {
                        if (value === CUSTOM_PROVIDER) {
                          copyAsCustom()
                          return
                        }
                        field.onChange(value)
                        applyPreset(value)
                      }}
                    />
                  )}
                />
                {isPresetMode ? (
                  <div className="space-y-2 rounded-lg border p-3 text-xs text-muted-foreground">
                    <div className="space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-foreground">
                          {presetName(presetForCurrent)}
                        </span>
                        <span className="shrink-0 rounded border px-1.5 py-0.5 text-[10px]">
                          {presetKindLabel(presetForCurrent)}
                        </span>
                    </div>
                    <p className="text-foreground">{presetSummary(presetForCurrent)}</p>
                  </div>
                  {presetForCurrent.note ? (
                    <p className="text-amber-600 dark:text-amber-500">
                      {t(presetForCurrent.note)}
                    </p>
                  ) : null}
                  {presetForCurrent.doc ? (
                    <a
                      href={presetForCurrent.doc}
                      target="_blank"
                      rel="noreferrer"
                      className="inline-block underline underline-offset-2"
                    >
                      {t('providers.presetDoc')}
                    </a>
                  ) : null}
                  <p>
                    {(presetForCurrent.api_formats ?? [])
                      .map((format) => t(`providers.formats.${format}`, { defaultValue: format }))
                      .join(' · ') || t('providers.noFormats')}
                  </p>
                  <p>{t('providers.lockedHint')}</p>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="mt-1 w-full gap-2 text-foreground"
                    onClick={() => (isEdit ? setConfirmConvert(true) : copyAsCustom())}
                  >
                    <CopyIcon className="size-4" />
                    {isEdit ? t('providers.convertToCustom') : t('providers.copyAsCustom')}
                  </Button>
                </div>
              ) : null}
              {confirmConvert ? (
                <div className="space-y-2 rounded-lg border border-amber-500/40 bg-amber-500/10 p-3 text-xs">
                  <p>{t('providers.convertConfirm')}</p>
                  <div className="flex gap-2">
                    <Button type="button" size="sm" onClick={copyAsCustom}>
                      {t('providers.convertConfirmYes')}
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => setConfirmConvert(false)}
                    >
                      {t('common.cancel')}
                    </Button>
                  </div>
                </div>
              ) : null}
              </div>

              {!isPresetMode ? (
                <div className="space-y-2">
                  <Label htmlFor="key-provider">{t('providers.providerKey')}</Label>
                  <Input id="key-provider" placeholder="my-proxy" {...register('provider')} />
                  <p className="text-xs text-muted-foreground">{t('providers.providerKeyHint')}</p>
                  {originPresetKey && providerValue !== `${originPresetKey}-custom` ? (
                    <p className="text-xs text-amber-600 dark:text-amber-500">
                      {t('providers.detachedFromPreset', { preset: originPresetKey })}
                    </p>
                  ) : null}
                </div>
              ) : null}

              <div className="space-y-2">
                <Label htmlFor="key-base-url">{t('providers.baseUrl')}</Label>
                {varDecls.length ? (
                  <div className="space-y-2 rounded-lg border p-3">
                    {varDecls.map((variable) => (
                      <div key={variable.name} className="space-y-1">
                        <Label htmlFor={`var-${variable.name}`} className="text-xs">
                          {t(variable.label ?? variable.name, { defaultValue: variable.name })}
                          {variable.required ? ' *' : ''}
                        </Label>
                        <Input
                          id={`var-${variable.name}`}
                          placeholder={variable.example ?? ''}
                          value={varValue(variable.name)}
                          onChange={(event) => {
                            const value = event.target.value
                            setVars((current) => ({ ...current, [variable.name]: value }))
                            setPathVars((current) => ({ ...current, [variable.name]: value }))
                          }}
                        />
                        {variable.kind === 'host-only' || variable.kind === 'full-url' ? (
                          <p className="text-xs text-muted-foreground">
                            {t(`providers.varKinds.${variable.kind}`)}
                          </p>
                        ) : null}
                      </div>
                    ))}
                  </div>
                ) : null}
                <Input
                  id="key-base-url"
                  value={expandedBaseUrl ?? ''}
                  readOnly={presetLocked}
                  placeholder={t('providers.baseUrlPlaceholder')}
                  {...(presetLocked ? {} : register('base_url'))}
                />
                <p className="text-xs text-muted-foreground">{t('providers.baseUrlHint')}</p>
              </div>

              {!isPresetMode ? (
                <div className="space-y-2">
                  <Label>{t('providers.authScheme')}</Label>
                  {forcedAuth ? (
                    <p className="text-sm text-muted-foreground">
                      {t(`providers.authSchemes.${forcedAuth}`)}（{t('providers.authLockedByCredential')}）
                    </p>
                  ) : (
                  <Controller
                    control={control}
                    name="auth_scheme"
                    render={({ field }) => (
                      <Select value={field.value} onValueChange={field.onChange}>
                        <SelectTrigger className="w-full">
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {API_KEY_SCHEMES.map((scheme) => (
                            <SelectItem key={scheme} value={scheme}>
                              {t(`providers.authSchemes.${scheme}`)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    )}
                  />
                  )}
                  <Label>{t('providers.apiFormats')}</Label>
                  <Controller
                    control={control}
                    name="api_formats"
                    render={({ field }) => {
                      const selected = field.value ?? []
                      const toggle = (format, checked) => {
                        const next = selected.filter((item) => item !== format)
                        if (checked) next.push(format)
                        field.onChange(next)
                      }
                      return (
                        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                          {VISIBLE_FORMATS.map((format) => (
                            <label key={format} className="flex items-center gap-2 text-sm">
                              <input
                                type="checkbox"
                                className="size-4 accent-foreground"
                                checked={selected.includes(format)}
                                onChange={(event) => toggle(format, event.target.checked)}
                              />
                              {t(`providers.formats.${format}`)}
                            </label>
                          ))}
                        </div>
                      )
                    }}
                  />
                </div>
              ) : null}

              <div className="space-y-2">
                <Label htmlFor="key-credential">
                  {t('providers.credential')}
                  {isEdit ? (
                    <span className="ml-2 text-muted-foreground">({t('providers.apiKeyKeepHint')})</span>
                  ) : null}
                </Label>
                <textarea
                  id="key-credential"
                  ref={credentialRef}
                  className="min-h-24 w-full rounded-md border border-input bg-transparent px-3 py-2 font-mono text-xs shadow-sm outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  placeholder={
                    isEdit && providerKey?.credential_masked
                      ? t('providers.credentialCurrent', { masked: providerKey.credential_masked })
                      : t('providers.credentialPlaceholder')
                  }
                  {...register('credential')}
                />
                <p className="text-xs text-muted-foreground">
                  {serviceAccount
                    ? t('providers.credentialServiceAccount', {
                        masked: providerKey?.credential_masked ?? '-',
                      })
                    : awsCredentials
                      ? t('providers.credentialAws', {
                          masked: providerKey?.credential_masked ?? '-',
                        })
                      : t('providers.credentialHint')}
                </p>
                {credentialUnsupported ? (
                  <p className="text-xs text-muted-foreground">{t('providers.credentialUnsupported')}</p>
                ) : null}
                {serverError ? <p className="text-sm text-destructive">{serverError}</p> : null}
              </div>

              <div className="space-y-2">
                <Label htmlFor="key-name">
                  {t('providers.name')} <span className="text-muted-foreground">({t('common.optional')})</span>
                </Label>
                <Input id="key-name" {...register('name')} />
              </div>

              <div className="space-y-2">
                <Label htmlFor="key-priority">{t('providers.priority')}</Label>
                <Input id="key-priority" type="number" min="1" max="1000" {...register('priority')} />
                <p className="text-xs text-muted-foreground">{t('providers.priorityHint')}</p>
              </div>

              <div className="space-y-2">
                <div className="flex items-center justify-between gap-2">
                  <Label>{t('providers.models')}</Label>
                  <div className="flex items-center gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => appendModel({ name: '', upstream: '', renamed: false })}
                    >
                      <PlusIcon className="size-4" />
                      {t('providers.addModel')}
                    </Button>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={fetchModels}
                      disabled={discover.isPending || credentialUnsupported}
                      title={credentialUnsupported ? t('providers.credentialUnsupported') : undefined}
                    >
                      {discover.isPending ? t('providers.fetching') : t('providers.fetchModels')}
                    </Button>
                  </div>
                </div>
                <p className="text-xs text-muted-foreground">{t('providers.modelsHint')}</p>
                {modelFields.length ? (
                  <div className="max-h-56 overflow-y-auto">
                    <div className="space-y-2 px-1">
                      {modelFields.map((field, index) => {
                        const renamed = modelsValue?.[index]?.renamed ?? false
                        return (
                          <div key={field.id} className="flex items-start gap-2">
                            {/* 始终占两等分：单列时输入框只占左半格，与展开后单列宽度一致。 */}
                            <div className="grid min-w-0 flex-1 grid-cols-2 gap-2">
                              {renamed ? (
                                <>
                                  <div className="min-w-0 space-y-1 rounded-lg border border-dashed p-2">
                                    <span className="text-xs font-medium text-muted-foreground">
                                      {t('providers.modelUpstream')}
                                    </span>
                                    <Input
                                      placeholder={t('providers.modelUpstreamPlaceholder')}
                                      {...register(`models.${index}.upstream`)}
                                    />
                                  </div>
                                  <div className="min-w-0 space-y-1 rounded-lg border border-dashed p-2">
                                    <span className="text-xs font-medium text-muted-foreground">
                                      {t('providers.modelName')}
                                    </span>
                                    <Input
                                      placeholder={t('providers.modelNamePlaceholder')}
                                      {...register(`models.${index}.name`)}
                                    />
                                  </div>
                                </>
                              ) : (
                                <Input
                                  placeholder={t('providers.modelIdPlaceholder')}
                                  {...register(`models.${index}.name`)}
                                />
                              )}
                            </div>
                            <div
                              className={`flex shrink-0 items-center gap-1 ${renamed ? 'mt-7' : ''}`}
                            >
                              {renamed ? (
                                <Button
                                  type="button"
                                  variant="ghost"
                                  size="icon-sm"
                                  title={t('providers.modelUnrename')}
                                  aria-label={t('providers.modelUnrename')}
                                  onClick={() => unrenameModel(index)}
                                >
                                  <Undo2Icon className="size-4" />
                                </Button>
                              ) : (
                                <Button
                                  type="button"
                                  variant="ghost"
                                  size="icon-sm"
                                  title={t('providers.modelRename')}
                                  aria-label={t('providers.modelRename')}
                                  onClick={() => renameModel(index)}
                                >
                                  <PencilIcon className="size-4" />
                                </Button>
                              )}
                              <Button
                                type="button"
                                variant="ghost"
                                size="icon-sm"
                                onClick={() => removeModel(index)}
                                aria-label={t('providers.removeModel')}
                              >
                                <TrashIcon className="size-4" />
                              </Button>
                            </div>
                          </div>
                        )
                      })}
                    </div>
                  </div>
                ) : (
                  <p className="text-xs text-muted-foreground">{t('providers.modelsEmpty')}</p>
                )}
              </div>

              <div className="flex items-center justify-between gap-4 rounded-lg border p-3">
                <div className="space-y-0.5">
                  <Label>{t('providers.status')}</Label>
                  <p className="text-xs text-muted-foreground">{t('providers.statusHint')}</p>
                </div>
                <Controller
                  control={control}
                  name="status"
                  render={({ field }) => (
                    <Switch
                      checked={field.value === 'active'}
                      onCheckedChange={(checked) => field.onChange(checked ? 'active' : 'inactive')}
                      aria-label={t('providers.status')}
                    />
                  )}
                />
              </div>
            </div>
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              {t('common.cancel')}
            </Button>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? t('common.saving') : t('common.save')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
