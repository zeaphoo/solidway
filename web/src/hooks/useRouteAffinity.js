import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

/** 路由会话记录（`last_used` 策略的读取依据；其它策略下仅作诊断）。 */
export function useRouteAffinity(routeId) {
  return useQuery({
    queryKey: ['route-affinity', routeId],
    queryFn: () => request(`/routes/${routeId}/affinity`),
    enabled: Boolean(routeId),
    refetchInterval: 10_000,
  })
}

/** 清除会话记录（可按模型 + 会话标识限定；都不传则清空该路由）。 */
export function useClearRouteAffinity(routeId) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (target) => {
      const params = new URLSearchParams()
      if (target?.model) params.set('model', target.model)
      if (target?.session) params.set('session', target.session)
      const suffix = params.toString()
      return request(`/routes/${routeId}/affinity${suffix ? `?${suffix}` : ''}`, {
        method: 'DELETE',
      })
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['route-affinity', routeId] })
    },
  })
}
