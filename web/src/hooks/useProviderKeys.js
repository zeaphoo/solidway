import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

export function useProviderPresets() {
  return useQuery({
    queryKey: ['providers'],
    queryFn: () => request('/providers'),
    staleTime: Infinity,
  })
}

export function useProviderKeys(routeId) {
  return useQuery({
    queryKey: ['provider-keys', routeId],
    queryFn: () => request(`/routes/${routeId}/provider-keys`),
    enabled: Boolean(routeId),
  })
}

export function useCreateProviderKey(routeId) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body) => request(`/routes/${routeId}/provider-keys`, { method: 'POST', body }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['provider-keys', routeId] }),
  })
}

export function useUpdateProviderKey(routeId) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }) => request(`/provider-keys/${id}`, { method: 'PATCH', body }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['provider-keys', routeId] }),
  })
}

export function useDeleteProviderKey(routeId) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id) => request(`/provider-keys/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['provider-keys', routeId] }),
  })
}
