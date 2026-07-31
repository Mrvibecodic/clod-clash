import { useEffect, useState } from 'react'
import useSWR from 'swr'

import { useVisibility } from '@/hooks/use-visibility'
import { getConnectSessionStart } from '@/services/cmds'

/**
 * Seconds since the Connect targets last came up — the session timer under
 * the Connect button.
 *
 * The baseline lives in the backend (`get_connect_session_start`): connects
 * and disconnects also happen from the settings page and the tray while this
 * hook is unmounted, so a frontend-remembered start would go stale. (The
 * separate `get_app_uptime` is the *application* uptime in milliseconds —
 * a different thing entirely.)
 *
 * The 1 s tick runs only while connected and the window is visible: a timer
 * nobody can see is not worth waking the CPU for. Hiding the window freezes
 * the display, and the first tick after showing it again catches it up.
 */
export const useSessionUptime = (connected: boolean): number | undefined => {
  const visible = useVisibility()
  const [uptime, setUptime] = useState<number>()

  // Refetch on every connect edge and on remount, so the baseline follows
  // whatever the backend actually recorded.
  const { data: sessionStartMs } = useSWR(
    connected ? 'getConnectSessionStart' : null,
    getConnectSessionStart,
    { revalidateOnFocus: true },
  )

  useEffect(() => {
    if (!connected || sessionStartMs == null || !visible) return

    const tick = () =>
      setUptime(Math.max(0, (Date.now() - sessionStartMs) / 1000))

    // The first tick goes through a zero timeout rather than a direct call:
    // a synchronous set inside an effect forces an extra render pass.
    const firstTick = window.setTimeout(tick, 0)
    const interval = window.setInterval(tick, 1000)
    return () => {
      window.clearTimeout(firstTick)
      window.clearInterval(interval)
    }
  }, [connected, sessionStartMs, visible])

  // Derived, not effect-set: a disconnect (or a not-yet-loaded baseline)
  // reads as "no timer" without an extra state write.
  return connected && sessionStartMs != null ? uptime : undefined
}
