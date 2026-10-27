import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

export function useSettings(enabled = true) {
  return useQuery({ queryKey: ['settings'], queryFn: () => request('/settings'), enabled })
}

export function useAccess() {
  return useQuery({ queryKey: ['access'], queryFn: () => request('/access') })
}

/** 内置 HTTPS 状态（仅 admin）：证书域名、状态、到期与端口映射提示。 */
export function useTlsStatus(enabled = true) {
  return useQuery({
    queryKey: ['tls-status'],
    queryFn: () => request('/tls/status'),
    enabled,
    staleTime: 0,
    refetchInterval: (query) => {
      const state = query.state.data?.cert?.state
      // 等待签发 / 已暂停时持续轮询，证书可用后放慢。
      return state === 'ok' ? 30_000 : 5_000
    },
  })
}

/** 立即重试一次证书签发 / 续期（异步受理）。 */
export function useTlsRetry() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => request('/tls/retry', { method: 'POST' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['tls-status'] }),
  })
}

export function useUpdateSettings() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body) => request('/settings', { method: 'PATCH', body }),
    onSuccess: (data) => {
      // 响应是 `AllSettings` 的扁平字段 + `restart_required`。
      const { restart_required: _restartRequired, ...settings } = data ?? {}
      queryClient.setQueryData(['settings'], settings)
      queryClient.invalidateQueries({ queryKey: ['access'] })
      queryClient.invalidateQueries({ queryKey: ['tls-status'] })
    },
  })
}
