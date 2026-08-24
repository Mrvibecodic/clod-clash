import dayjs from 'dayjs'
import { memo, useSyncExternalStore } from 'react'

import { useVisibility } from '@/hooks/use-visibility'

type RelativeTimeListener = () => void

let currentTime = Date.now()
let timerId: number | null = null
const listeners = new Set<RelativeTimeListener>()

const startTimer = () => {
  if (timerId !== null) return

  currentTime = Date.now()
  timerId = window.setInterval(() => {
    currentTime = Date.now()
    listeners.forEach((listener) => {
      listener()
    })
  }, 5_000)
}

const stopTimer = () => {
  if (listeners.size > 0 || timerId === null) return

  window.clearInterval(timerId)
  timerId = null
}

const subscribeRelativeTime = (listener: RelativeTimeListener) => {
  listeners.add(listener)
  startTimer()

  return () => {
    listeners.delete(listener)
    stopTimer()
  }
}

const subscribeNothing = () => () => {
  return
}

const getRelativeTimeSnapshot = () => currentTime

interface RelativeTimeProps {
  start: string
}

export const RelativeTime = memo(function RelativeTime({
  start,
}: RelativeTimeProps) {
  const visible = useVisibility()
  const now = useSyncExternalStore(
    visible ? subscribeRelativeTime : subscribeNothing,
    getRelativeTimeSnapshot,
    getRelativeTimeSnapshot,
  )
  return <>{dayjs(start).from(now)}</>
})
