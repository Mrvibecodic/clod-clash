import { useMemo } from 'react'

/**
 * clod: the bottom drawer must never cover the connect button — from an open
 * server list one still has to be able to disconnect.
 *
 * The ceiling is measured off the button itself (`data-connect-anchor`) rather
 * than expressed in `vh`: the simple and the advanced screen put the button at
 * different heights, and a user-set font scale moves it again. When the anchor
 * is missing (a screen without the button, an early paint) the old viewport
 * share is used instead, so the drawer degrades to its previous behaviour.
 */
const FALLBACK_RATIO = 0.8
/** Breathing room between the button and the drawer's top edge. */
const ANCHOR_GAP = 12
/** Never squeeze the drawer below this: a list of two rows is still useful. */
const MIN_HEIGHT = 240

/** Ставится литералом на блок кнопки Connect в `connect-button.tsx`. */
const CONNECT_ANCHOR_ATTRIBUTE = 'data-connect-anchor'

const measureCap = () => {
  const viewport = window.innerHeight
  const fallback = viewport * FALLBACK_RATIO
  const anchor = document.querySelector(`[${CONNECT_ANCHOR_ATTRIBUTE}]`)
  if (!anchor) return fallback
  const { bottom } = anchor.getBoundingClientRect()
  // A hidden or not-yet-laid-out anchor reports zeroes; trust nothing that
  // sits outside the viewport either.
  if (bottom <= 0 || bottom >= viewport) return fallback
  return Math.max(MIN_HEIGHT, viewport - bottom - ANCHOR_GAP)
}

/**
 * Maximum height the server drawer may take, as a CSS length.
 *
 * Measured while rendering rather than in an effect: the anchor belongs to the
 * screen underneath and was laid out long before the drawer opened, so there
 * is nothing to wait for — and seeding state from an effect would cost an
 * extra render on every open.
 *
 * The pixel value is taken once per open and paired with `min(…, 80vh)`, so a
 * window resized while the list is open can never end up with a drawer taller
 * than the window itself: the ceiling only goes stale, never wrong.
 */
export const useDrawerCapHeight = (open: boolean) =>
  useMemo(
    () => (open ? `min(${Math.round(measureCap())}px, 80vh)` : '80vh'),
    [open],
  )
