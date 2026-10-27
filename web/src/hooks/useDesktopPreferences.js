import { useMutation, useQuery } from '@tanstack/react-query'

import { request } from '@/lib/api'

/**
 * 桌面壳偏好（仅 desktop 模式可用）。
 *
 * 偏好与回执都由桌面壳写入 `data_root/desktop.toml` / `desktop.status.json`，
 * 服务端只读：`prefs` 在壳落盘前仍是旧值，界面按 `status.apply_id` 等回执。
 */
export function useDesktopPreferences() {
  return useQuery({
    queryKey: ['desktop-prefs'],
    queryFn: () => request('/app/desktop'),
    staleTime: 0,
    // 同步托盘侧或其他窗口的改动（设计口径：5 秒内）。
    refetchInterval: 5000,
  })
}

/** 请求桌面壳应用偏好；返回 `{ apply_id, pending }`，结果由回执确认。 */
export function useUpdateDesktopPreferences() {
  return useMutation({
    mutationFn: (body) => request('/app/desktop', { method: 'PUT', body }),
  })
}
