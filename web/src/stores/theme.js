import { create } from 'zustand'

const STORAGE_KEY = 'solidway-theme'

export const THEME_MODES = ['light', 'dark', 'system']

function systemPrefersDark() {
  if (typeof window === 'undefined' || !window.matchMedia) return false
  return window.matchMedia('(prefers-color-scheme: dark)').matches
}

function readStoredMode() {
  if (typeof window === 'undefined') return 'system'
  const stored = window.localStorage.getItem(STORAGE_KEY)
  return THEME_MODES.includes(stored) ? stored : 'system'
}

/** 把模式解析为实际生效主题（`system` 取系统偏好）。 */
export function resolveTheme(mode) {
  if (mode === 'light' || mode === 'dark') return mode
  return systemPrefersDark() ? 'dark' : 'light'
}

function applyTheme(theme) {
  if (typeof document === 'undefined') return
  document.documentElement.classList.toggle('dark', theme === 'dark')
}

const initialMode = readStoredMode()

export const useThemeStore = create((set) => ({
  mode: initialMode,
  theme: resolveTheme(initialMode),
  setMode: (mode) => {
    if (!THEME_MODES.includes(mode)) return
    if (typeof window !== 'undefined') window.localStorage.setItem(STORAGE_KEY, mode)
    const theme = resolveTheme(mode)
    applyTheme(theme)
    set({ mode, theme })
  },
}))

/**
 * 启动时应用主题，并在 `system` 模式下跟随系统偏好实时变化。
 * 已返回的解绑函数可用于清理。
 */
export function initTheme() {
  applyTheme(useThemeStore.getState().theme)

  if (typeof window === 'undefined' || !window.matchMedia) return () => {}

  const mql = window.matchMedia('(prefers-color-scheme: dark)')
  const onChange = () => {
    if (useThemeStore.getState().mode !== 'system') return
    const theme = resolveTheme('system')
    applyTheme(theme)
    useThemeStore.setState({ theme })
  }
  mql.addEventListener('change', onChange)

  return () => mql.removeEventListener('change', onChange)
}
