import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect } from 'react'

import { request } from '@/lib/api'
import { clearStoredToken } from '@/lib/desktopLogin'
import { useSessionStore } from '@/stores/session'

/** 读取当前会话；401 视为未认证。 */
export function useSession() {
  const setUser = useSessionStore((s) => s.setUser)
  const clear = useSessionStore((s) => s.clear)

  const query = useQuery({
    queryKey: ['session'],
    queryFn: () => request('/auth/me'),
    retry: false,
    staleTime: 60_000,
  })

  useEffect(() => {
    if (query.data) setUser(query.data)
  }, [query.data, setUser])

  useEffect(() => {
    if (query.isError) clear()
  }, [query.isError, clear])

  return query
}

export function useLogin() {
  const queryClient = useQueryClient()
  const setUser = useSessionStore((s) => s.setUser)

  return useMutation({
    mutationFn: (values) => request('/auth/login', { method: 'POST', body: values }),
    onSuccess: (user) => {
      setUser(user)
      queryClient.setQueryData(['session'], user)
    },
  })
}

export function useLogout() {
  const queryClient = useQueryClient()
  const clear = useSessionStore((s) => s.clear)

  return useMutation({
    mutationFn: () => request('/auth/logout', { method: 'POST' }),
    onSettled: () => {
      // 显式退出 = 明确不想被自动登录；托盘「以管理员自动登录」可重新注入令牌。
      clearStoredToken()
      clear()
      queryClient.clear()
    },
  })
}
