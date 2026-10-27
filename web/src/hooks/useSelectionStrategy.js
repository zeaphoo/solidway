import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

/** 路由选号策略 + 实时指标快照（10 秒刷新）。 */
export function useSelectionStrategy(routeId) {
  return useQuery({
    queryKey: ['selection-strategy', routeId],
    queryFn: () => request(`/routes/${routeId}/selection-strategy`),
    enabled: Boolean(routeId),
    refetchInterval: 10_000,
  })
}

/** 保存策略；保存后即时生效（无需重启）。 */
export function useUpdateSelectionStrategy(routeId) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body) =>
      request(`/routes/${routeId}/selection-strategy`, { method: 'PUT', body }),
    onSuccess: (data) => {
      queryClient.setQueryData(['selection-strategy', routeId], data)
    },
  })
}
