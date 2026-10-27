import { useMutation } from '@tanstack/react-query'

import { request } from '@/lib/api'

/** 调上游 `GET {base_url}/models` 取模型清单；失败返回空清单 + error。 */
export function useDiscoverModels() {
  return useMutation({
    mutationFn: (body) => request('/models/discover', { method: 'POST', body }),
  })
}
