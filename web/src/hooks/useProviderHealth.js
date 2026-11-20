import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'
import { pollEvery } from '@/lib/polling'

export function useProviderHealth() {
  return useQuery({
    queryKey: ['provider-health'],
    queryFn: () => request('/provider-health'),
    refetchInterval: pollEvery(15_000),
  })
}

/** 手动解除某 provider 凭据的健康标记（仅 admin）。 */
export function useClearProviderHealth() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (providerId) => request(`/provider-health/${providerId}`, { method: 'DELETE' }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['provider-health'] })
      queryClient.invalidateQueries({ queryKey: ['provider-incidents'] })
      queryClient.invalidateQueries({ queryKey: ['admin-provider-incidents'] })
      queryClient.invalidateQueries({ queryKey: ['provider-incident-stats'] })
    },
  })
}
