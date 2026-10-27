import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'
import { useSessionStore } from '@/stores/session'

export function useUpdateProfile() {
  const queryClient = useQueryClient()
  const setUser = useSessionStore((s) => s.setUser)
  return useMutation({
    mutationFn: (body) => request('/auth/me', { method: 'PATCH', body }),
    onSuccess: (user) => {
      setUser(user)
      queryClient.setQueryData(['session'], user)
    },
  })
}

export function useChangePassword() {
  return useMutation({
    mutationFn: (body) => request('/auth/password', { method: 'POST', body }),
  })
}

/** 是否存在待用引导令牌（公开接口，用于设置页自检）。 */
export function useBootstrapStatus() {
  return useQuery({
    queryKey: ['bootstrap'],
    queryFn: () => request('/auth/bootstrap'),
    retry: false,
    staleTime: 0,
  })
}

/** 引导完成：写入初始口令并直接获得会话（无需再登录）。 */
export function useBootstrap() {
  const queryClient = useQueryClient()
  const setUser = useSessionStore((s) => s.setUser)

  return useMutation({
    mutationFn: (body) => request('/auth/bootstrap', { method: 'POST', body }),
    onSuccess: (user) => {
      setUser(user)
      queryClient.setQueryData(['session'], user)
      queryClient.removeQueries({ queryKey: ['bootstrap'] })
    },
  })
}

export function useSystemInfo() {
  return useQuery({
    queryKey: ['system'],
    queryFn: () => request('/system'),
    staleTime: 60_000,
  })
}

/** 当前版本（任意角色可见；不随会话变化，永久缓存）。 */
export function useVersion() {
  return useQuery({
    queryKey: ['version'],
    queryFn: () => request('/version'),
    staleTime: Infinity,
    retry: false,
  })
}
