import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { request } from '@/lib/api'

/**
 * 运行模式与监听事实（`/api/app/info`）。
 *
 * 只含界面需要的信息：`mode`、`can_restart`、`version`、`listen`。
 * 注意：接口仅 admin 可读，非 admin 应传 `enabled = false`（版本号用 `/version`）。
 */
export function useAppInfo(enabled = true) {
  return useQuery({
    queryKey: ['app-info'],
    queryFn: () => request('/app/info'),
    staleTime: 0,
    enabled,
  })
}

/** 请求托管方重启（仅桌面模式可用；独立模式返回 409）。 */
export function useRestartApp() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => request('/app/restart', { method: 'POST' }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['app-info'] }),
  })
}
