/** 桌面自动登录令牌的本地存取（只在当前 webview 会话内有效）。 */

/*
 * 不变量：令牌只在两处写入 —— `/desktop` 落页换取会话成功、`RequireAuth` 续会话成功；
 * 唯一清除点是显式退出登录（`useLogout`）。所以「本机有令牌」等价于
 * 「当前会话由桌面进程令牌签发」：想用口令登录必先退出，而退出必清令牌。
 * 该判别与「是否运行在桌面壳里」无关——桌面壳内用口令登录的员工不应被误判。
 */

export const DESKTOP_TOKEN_KEY = 'solidway.desktop_token'
/** 「这个浏览器是桌面壳的 webview」——由 `/desktop` 落地页写入，外部浏览器永远没有。 */
export const DESKTOP_SHELL_KEY = 'solidway.desktop_shell'

/** 读取并清除 URL fragment（令牌不留在地址栏 / 历史里）。 */
export function takeFragmentToken() {
  const raw = window.location.hash.replace(/^#/, '')
  window.history.replaceState(null, '', window.location.pathname + window.location.search)
  return raw ? decodeURIComponent(raw) : ''
}

export function readStoredToken() {
  try {
    return window.sessionStorage.getItem(DESKTOP_TOKEN_KEY) || ''
  } catch {
    return ''
  }
}

export function storeToken(token) {
  try {
    if (token) window.sessionStorage.setItem(DESKTOP_TOKEN_KEY, token)
  } catch {
    /* 隐私模式等场景忽略：令牌只影响自动续期 */
  }
}

export function markDesktopShell() {
  try {
    window.localStorage.setItem(DESKTOP_SHELL_KEY, '1')
  } catch {
    /* 隐私模式等场景忽略 */
  }
}

export function isDesktopShell() {
  try {
    return window.localStorage.getItem(DESKTOP_SHELL_KEY) === '1'
  } catch {
    return false
  }
}

/**
 * 当前会话是否由桌面进程令牌自动签发（免登录）。
 *
 * 用途：桌面免登录会话不涉及口令，个人页应据此不渲染口令表单。
 * 注意：不要叠加 `isDesktopShell()`，那会把桌面壳内以口令登录的用户一起命中。
 */
export function isTokenSession() {
  return Boolean(readStoredToken())
}

export function clearStoredToken() {
  try {
    window.sessionStorage.removeItem(DESKTOP_TOKEN_KEY)
  } catch {
    /* 同上 */
  }
}
