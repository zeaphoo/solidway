import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query'

let unauthorizedHandler = () => {}

/** 由 App 注入：收到 401 时清除本地会话并跳转登录。 */
export function setUnauthorizedHandler(handler) {
  unauthorizedHandler = handler
}

function onError(error) {
  if (error?.status === 401) unauthorizedHandler()
}

export const queryClient = new QueryClient({
  queryCache: new QueryCache({ onError }),
  mutationCache: new MutationCache({ onError }),
  defaultOptions: {
    queries: {
      retry: (failureCount, error) => {
        if (error?.status >= 400 && error?.status < 500) return false
        return failureCount < 2
      },
      refetchOnWindowFocus: false,
      staleTime: 30_000,
    },
  },
})
