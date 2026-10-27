import { useQuery } from '@tanstack/react-query'

import { request } from '@/lib/api'

/** 绑定地址候选（回环 / 通配 / 本机出网地址），仅 admin 可用。 */
export function useNetworkAddresses(enabled = true) {
  return useQuery({
    queryKey: ['network-addresses'],
    queryFn: () => request('/network/addresses'),
    enabled,
    staleTime: 60_000,
  })
}
