import i18n from 'i18next'
import LanguageDetector from 'i18next-browser-languagedetector'
import { initReactI18next } from 'react-i18next'

import enUS from '@/locales/en-US.json'
import helpEnUS from '@/locales/help/en-US.json'
import helpZhCN from '@/locales/help/zh-CN.json'
import zhCN from '@/locales/zh-CN.json'

export const SUPPORTED_LANGS = ['zh-CN', 'en-US']
export const LANG_STORAGE_KEY = 'solidway-lang'
export const DEFAULT_LANG = 'zh-CN'

/**
 * 把检测到的语言归一化到我们实际提供的语言包。
 * 注意：不要用 `nonExplicitSupportedLngs`——它会把 `zh-CN` 解析到基语言
 * `zh` 去找资源包，而 `zh` 不存在，导致 `t()` 直接返回 key。
 */
export function normalizeLanguage(lng) {
  if (!lng) return DEFAULT_LANG
  const lower = String(lng).toLowerCase()
  if (lower.startsWith('zh')) return 'zh-CN'
  if (lower.startsWith('en')) return 'en-US'
  return lng
}

i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      // 帮助正文体量大，单独成文件后与主文案合并，避免主文件继续膨胀。
      'zh-CN': { translation: { ...zhCN, help: helpZhCN } },
      'en-US': { translation: { ...enUS, help: helpEnUS } },
    },
    supportedLngs: SUPPORTED_LANGS,
    fallbackLng: DEFAULT_LANG,
    load: 'currentOnly',
    detection: {
      order: ['localStorage', 'navigator'],
      caches: ['localStorage'],
      lookupLocalStorage: LANG_STORAGE_KEY,
      convertDetectedLanguage: normalizeLanguage,
    },
    interpolation: { escapeValue: false },
  })

export default i18n
