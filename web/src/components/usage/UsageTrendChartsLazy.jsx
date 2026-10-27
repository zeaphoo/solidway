import { Suspense, lazy } from 'react'

import { Skeleton } from '@/components/ui/skeleton'

/** 图表组件按需加载：Recharts 与图表代码只在统计区出现时下载。 */
const UsageTrendCharts = lazy(() => import('./UsageTrendCharts'))

export default function UsageTrendChartsLazy(props) {
  return (
    <Suspense fallback={<Skeleton className={props.compact ? 'h-40 w-full' : 'h-56 w-full'} />}>
      <UsageTrendCharts {...props} />
    </Suspense>
  )
}
