import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

function incidentSuffix(params) {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== '') search.set(key, value)
  }
  return search.toString() ? `?${search}` : ''
}

/** 故障记录（当前用户路由范围；未解决优先，按最近发生排序）。 */
export function useIncidents(params = {}, enabled = true) {
  const suffix = incidentSuffix(params)
  return useQuery({
    queryKey: ['provider-incidents', suffix],
    queryFn: () => request(`/provider-incidents${suffix}`),
    enabled,
    refetchInterval: 15_000,
  })
}

/** 全站故障记录（仅 admin，含已关闭历史）。 */
export function useAdminIncidents(params = {}, enabled = true) {
  const suffix = incidentSuffix(params)
  return useQuery({
    queryKey: ['admin-provider-incidents', suffix],
    queryFn: () => request(`/admin/provider-incidents${suffix}`),
    enabled,
    refetchInterval: 15_000,
  })
}

/** 按 incident id 关闭未解决故障（admin；不依赖 Provider 是否仍存在）。 */
export function useCloseIncident() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id) =>
      request(`/admin/provider-incidents/${id}/close`, { method: 'POST' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['provider-incidents'] })
      queryClient.invalidateQueries({ queryKey: ['admin-provider-incidents'] })
      queryClient.invalidateQueries({ queryKey: ['provider-health'] })
    },
  })
}

/** 各 Provider 近 24h 故障次数与累计时长。 */
export function useIncidentStats(enabled = true) {
  return useQuery({
    queryKey: ['provider-incident-stats'],
    queryFn: () => request('/provider-incidents/stats'),
    enabled,
    refetchInterval: 30_000,
  })
}
