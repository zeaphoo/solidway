import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

export function useApiKeys() {
  return useQuery({ queryKey: ['api-keys'], queryFn: () => request('/api-keys') })
}

export function useCreateApiKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ routeId, name, expires_at }) =>
      request(`/routes/${routeId}/api-keys`, {
        method: 'POST',
        body: { name: name || undefined, expires_at },
      }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['api-keys'] }),
  })
}

export function useUpdateApiKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }) => request(`/api-keys/${id}`, { method: 'PATCH', body }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['api-keys'] }),
  })
}

export function useDeleteApiKey() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id) => request(`/api-keys/${id}`, { method: 'DELETE' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['api-keys'] }),
  })
}

export function useRouteApiKeys(routeId) {
  return useQuery({
    queryKey: ['api-keys', routeId],
    queryFn: () => request(`/routes/${routeId}/api-keys`),
    enabled: Boolean(routeId),
  })
}
