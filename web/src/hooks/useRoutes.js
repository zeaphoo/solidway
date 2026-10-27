import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

export function useRoutes({ includeArchived = false } = {}) {
  return useQuery({
    queryKey: ['routes', { includeArchived }],
    queryFn: () => request(`/routes${includeArchived ? '?include_archived=true' : ''}`),
  })
}

export function useRoute(id) {
  return useQuery({
    queryKey: ['route', id],
    queryFn: () => request(`/routes/${id}`),
    enabled: Boolean(id),
  })
}

export function useCreateRoute() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body) => request('/routes', { method: 'POST', body }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['routes'] }),
  })
}

export function useUpdateRoute() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }) => request(`/routes/${id}`, { method: 'PATCH', body }),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: ['routes'] })
      queryClient.invalidateQueries({ queryKey: ['route', variables.id] })
    },
  })
}

export function useArchiveRoute() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id) => request(`/routes/${id}/archive`, { method: 'POST' }),
    onSuccess: (_data, id) => {
      queryClient.invalidateQueries({ queryKey: ['routes'] })
      queryClient.invalidateQueries({ queryKey: ['route', id] })
    },
  })
}

export function useTransferRoute() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ownerId }) =>
      request(`/routes/${id}/transfer`, { method: 'POST', body: { owner_id: ownerId } }),
    onSuccess: (_data, variables) => {
      queryClient.invalidateQueries({ queryKey: ['routes'] })
      queryClient.invalidateQueries({ queryKey: ['route', variables.id] })
      queryClient.invalidateQueries({ queryKey: ['users'] })
    },
  })
}

/** 兼容旧调用：删除即归档。 */
export function useDeleteRoute() {
  return useArchiveRoute()
}
