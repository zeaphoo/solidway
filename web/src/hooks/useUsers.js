import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

export function useUsers({ includeArchived = false, enabled = true } = {}) {
  return useQuery({
    queryKey: ['users', { includeArchived }],
    queryFn: () => request(`/users${includeArchived ? '?include_archived=true' : ''}`),
    enabled,
  })
}

export function useCreateUser() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (body) => request('/users', { method: 'POST', body }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['users'] }),
  })
}

export function useUpdateUser() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, body }) => request(`/users/${id}`, { method: 'PATCH', body }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['users'] }),
  })
}

export function useResetUserPassword() {
  return useMutation({
    mutationFn: ({ id, password }) =>
      request(`/users/${id}/password`, { method: 'POST', body: { password: password || undefined } }),
  })
}

export function useArchiveUser() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id) => request(`/users/${id}/archive`, { method: 'POST' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['users'] }),
  })
}

export function useOffboardUser() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, transferTo }) =>
      request(`/users/${id}/offboard`, {
        method: 'POST',
        body: { transfer_to: transferTo || undefined },
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['users'] })
      queryClient.invalidateQueries({ queryKey: ['routes'] })
    },
  })
}

/** 兼容旧调用：删除即归档。 */
export function useDeleteUser() {
  return useArchiveUser()
}
