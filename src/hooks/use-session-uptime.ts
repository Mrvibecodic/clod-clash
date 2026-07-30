import { useEffect, useState } from 'react'

import { useVisibility } from '@/hooks/use-visibility'

/**
 * The moment the current session came up, shared between the simple and the
 * advanced screen so switching them does not restart the timer. Module-level
 * on purpose: there is no backend record of when the session started
 * (`get_app_uptime` is the *application* uptime, in milliseconds — a different
 * thing entirely), so the app remembers it itself and the timer resets with
 * the app.
 */
let sessionStartMs: number | undefined

/**
 * Seconds since the Connect targets last came up — the session timer under
 * the Connect button.
 *
 * The 1 s tick runs only while connected and the window is visible: a timer
 * nobody can see is not worth waking the CPU for. Hiding the window freezes
 * the display, and the first tick after showing it again catches it up.
 */
export const useSessionUptime = (connected: boolean): number | undefined => {
  const visible = useVisibility()
  const [uptime, setUptime] = useState<number>()

  useEffect(() => {
    if (!connected) {
      sessionStartMs = undefined
      return
    }

    const startMs = (sessionStartMs ??= Date.now())
    if (!visible) return

    const tick = () => setUptime((Date.now() - startMs) / 1000)

    // The first tick goes through a zero timeout rather than a direct call:
    // a synchronous set inside an effect forces an extra render pass.
    const firstTick = window.setTimeout(tick, 0)
    const interval = window.setInterval(tick, 1000)
    return () => {
      window.clearTimeout(firstTick)
      window.clearInterval(interval)
    }
  }, [connected, visible])

  return connected ? uptime : undefined
}
