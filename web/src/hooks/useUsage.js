import { useQuery } from '@tanstack/react-query'

import { request } from '@/lib/api'

function qs(params) {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value !== undefined && value !== null && value !== '') search.set(key, value)
  }
  const text = search.toString()
  return text ? `?${text}` : ''
}

const REFETCH_MS = 15_000

function endpoint(scope, name) {
  return scope === 'global' ? `/admin/usage/${name}` : `/usage/${name}`
}

function clean(params) {
  const { scope: _scope, enabled: _enabled, ...rest } = params
  return rest
}

function commonKey(params) {
  const { scope: _scope, enabled: _enabled, ...rest } = params
  return Object.entries(rest)
    .filter(([, value]) => value !== undefined && value !== null && value !== '')
    .sort(([a], [b]) => a.localeCompare(b))
}

export function useUsageSummary(params) {
  return useQuery({
    queryKey: ['usage', 'summary', ...commonKey(params)],
    queryFn: () => request(`${endpoint(params.scope, 'summary')}${qs(clean(params))}`),
    refetchInterval: REFETCH_MS,
    enabled: params.enabled !== false,
  })
}

export function useUsageTimeseries(params) {
  return useQuery({
    queryKey: ['usage', 'timeseries', ...commonKey(params)],
    queryFn: () => request(`${endpoint(params.scope, 'timeseries')}${qs(clean(params))}`),
    refetchInterval: REFETCH_MS,
    enabled: params.enabled !== false,
  })
}

export function useUsageBreakdown(params) {
  return useQuery({
    queryKey: ['usage', 'breakdown', ...commonKey(params)],
    queryFn: () => request(`${endpoint(params.scope, 'breakdown')}${qs(clean(params))}`),
    refetchInterval: REFETCH_MS,
    enabled: params.enabled !== false,
  })
}

export function useUsageRequests(params) {
  return useQuery({
    queryKey: ['usage', 'requests', ...commonKey(params)],
    queryFn: () => request(`${endpoint(params.scope, 'requests')}${qs(clean(params))}`),
    refetchInterval: REFETCH_MS,
    enabled: params.enabled !== false,
  })
}

export function useUsageConfig() {
  return useQuery({
    queryKey: ['usage', 'config'],
    queryFn: () => request('/usage/config'),
    staleTime: 5 * 60_000,
  })
}
