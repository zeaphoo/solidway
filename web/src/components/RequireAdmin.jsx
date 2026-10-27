import { Navigate } from 'react-router'

import { Skeleton } from '@/components/ui/skeleton'
import { useSession } from '@/hooks/useAuth'

/** 仅 admin 可访问；非 admin 重定向到路由页。 */
export default function RequireAdmin({ children }) {
  const { data: user, isPending } = useSession()

  if (isPending) {
    return (
      <div className="mx-auto max-w-5xl space-y-2">
        <Skeleton className="h-9 w-full" />
        <Skeleton className="h-9 w-full" />
      </div>
    )
  }

  if (user?.role !== 'admin') return <Navigate to="/routes" replace />

  return children
}
