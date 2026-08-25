import { useCallback, useEffect, useRef, useState } from 'react'

import { useVerge } from '@/hooks/use-verge'
import { useVisibility } from '@/hooks/use-visibility'
import { fitWindowToContent } from '@/services/cmds'
import { createStartupSettle } from '@/utils/window-settle'

const FIT_DEBOUNCE_MS = 120

const MINIMAL_HEIGHT = 520

const COMPACT_HYSTERESIS = 24

const SELF_RESIZE_GRACE_MS = 1200

const HEIGHT_MATCH_EPSILON = 3

const startup = createStartupSettle(Date.now())

export const isStartupWindowGrace = () => startup.isGrace(Date.now())

export const markStartupWindowSettled = () => startup.markSettled()

let selfResizeUntil = 0
let acceptAnyUntil = 0
const expectedHeights: number[] = []

export const markSelfWindowResize = (height?: number) => {
  selfResizeUntil = Date.now() + SELF_RESIZE_GRACE_MS
  if (height === undefined) {
    acceptAnyUntil = selfResizeUntil
    return
  }
  expectedHeights.push(Math.round(height))
  if (expectedHeights.length > 8) expectedHeights.shift()
}

export const isSelfWindowResize = (height: number) => {
  const now = Date.now()
  if (now < acceptAnyUntil) return true
  if (now >= selfResizeUntil) return false
  return expectedHeights.some(
    (expected) => Math.abs(expected - height) <= HEIGHT_MATCH_EPSILON,
  )
}

const measureContentHeight = (root: HTMLElement) => {
  const previous = root.style.height
  root.style.height = 'auto'
  const height = root.scrollHeight
  root.style.height = previous
  return height
}

let fitSuspended = false

export const suspendWindowFit = () => {
  fitSuspended = true
}

export const resumeWindowFit = () => {
  fitSuspended = false
}

const fitRequestListeners = new Set<() => void>()

export const requestWindowFit = () => {
  for (const listener of fitRequestListeners) listener()
}

export const useFitWindowToContent = () => {
  const { verge } = useVerge()
  const enabled = verge?.window_fit_content !== false
  const visible = useVisibility()

  const [root, setRoot] = useState<HTMLElement | null>(null)
  const [compact, setCompact] = useState(false)

  const visibleRef = useRef(visible)

  const compactRef = useRef(false)
  const normalHeightRef = useRef(0)
  const compactSavingRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  const applyFit = useCallback(async () => {
    if (!visibleRef.current) return
    if (!enabled || fitSuspended) {
      if (compactRef.current) {
        compactRef.current = false
        setCompact(false)
      }
      return
    }
    if (!root) return

    const chrome = Math.max(0, window.innerHeight - root.clientHeight)
    const desired = measureContentHeight(root) + chrome
    if (desired <= 0) return

    markSelfWindowResize(desired)
    startup.markFitAttempt(Date.now())
    const ceiling = await fitWindowToContent(desired).catch(() => 0)
    if (!ceiling) return
    markSelfWindowResize(Math.min(Math.max(desired, MINIMAL_HEIGHT), ceiling))

    if (compactRef.current) {
      if (normalHeightRef.current > desired) {
        compactSavingRef.current = normalHeightRef.current - desired
      }
      const normalGuess = desired + compactSavingRef.current
      if (normalGuess + COMPACT_HYSTERESIS <= ceiling) {
        compactRef.current = false
        setCompact(false)
      }
      return
    }

    normalHeightRef.current = desired
    if (desired > ceiling + 1) {
      compactRef.current = true
      setCompact(true)
    }
  }, [root, enabled])

  const schedule = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = setTimeout(() => {
      timerRef.current = undefined
      void applyFit()
    }, FIT_DEBOUNCE_MS)
  }, [applyFit])

  useEffect(() => {
    if (enabled) resumeWindowFit()
  }, [enabled])

  useEffect(() => {
    fitRequestListeners.add(schedule)
    return () => {
      fitRequestListeners.delete(schedule)
    }
  }, [schedule])

  useEffect(() => {
    const wasVisible = visibleRef.current
    visibleRef.current = visible
    if (!visible || wasVisible) return
    markSelfWindowResize()
    schedule()
  }, [visible, schedule])

  useEffect(() => {
    if (!root) return

    const observer = new ResizeObserver(schedule)
    const observeAll = () => {
      observer.disconnect()
      observer.observe(root)
      for (const child of Array.from(root.children)) observer.observe(child)
    }
    observeAll()

    const mutations = new MutationObserver(() => {
      observeAll()
      schedule()
    })
    mutations.observe(root, { childList: true })

    schedule()

    return () => {
      observer.disconnect()
      mutations.disconnect()
      if (timerRef.current) {
        clearTimeout(timerRef.current)
        timerRef.current = undefined
      }
    }
  }, [root, schedule])

  return { fitRef: setRoot, compact }
}
