import { useMutation, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'
import { clearStoredToken, storeToken } from '@/lib/desktopLogin'
import { useSessionStore } from '@/stores/session'

/**
 * 用桌面令牌换取管理员会话。
 *
 * 成功：写入会话缓存并记住令牌（会话过期时可以再换一次）；
 * 失败：仅 401 时丢弃令牌（网络错误保留，等下次重试）。
 */
export function useDesktopLogin() {
  const queryClient = useQueryClient()
  const setUser = useSessionStore((s) => s.setUser)

  return useMutation({
    mutationFn: async (token) => {
      const user = await request('/auth/desktop', { method: 'POST', body: { token } })
      storeToken(token)
      return user
    },
    onSuccess: (user) => {
      setUser(user)
      queryClient.setQueryData(['session'], user)
    },
    onError: (error) => {
      if (error?.status === 401) clearStoredToken()
    },
  })
}
