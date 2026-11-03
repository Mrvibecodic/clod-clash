import dayjs from 'dayjs'
import { memo } from 'react'

import { useNow } from '@/hooks/use-now'

interface RelativeTimeProps {
  start: string
}

export const RelativeTime = memo(function RelativeTime({
  start,
}: RelativeTimeProps) {
  const now = useNow(5_000)
  return <>{dayjs(start).from(now)}</>
})
