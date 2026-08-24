import { useMemo, useRef } from 'react'

const FALLBACK_RATIO = 0.8
const ANCHOR_GAP = 12
const MIN_HEIGHT = 240
const CONNECT_ANCHOR_ATTRIBUTE = 'data-connect-anchor'
const DEFAULT_CAP = '80vh'

const measureCap = () => {
  const viewport = window.innerHeight
  const fallback = viewport * FALLBACK_RATIO
  const anchor = document.querySelector(`[${CONNECT_ANCHOR_ATTRIBUTE}]`)
  if (!anchor) return fallback
  const { bottom } = anchor.getBoundingClientRect()
  if (bottom <= 0 || bottom >= viewport) return fallback
  return Math.max(MIN_HEIGHT, viewport - bottom - ANCHOR_GAP)
}

export const useDrawerCapHeight = (open: boolean) => {
  const capRef = useRef(DEFAULT_CAP)
  const measured = useMemo(
    () => (open ? `min(${Math.round(measureCap())}px, ${DEFAULT_CAP})` : null),
    [open],
  )
  if (measured) capRef.current = measured
  return capRef.current
}
