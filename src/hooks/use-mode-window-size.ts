import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useEffect, useRef } from 'react'

import { useSimpleMode } from '@/hooks/use-simple-mode'
import { saveWindowSizeForMode } from '@/services/cmds'

const SAVE_DEBOUNCE_MS = 800

/**
 * Keeps the per-mode window geometry fresh: after a manual resize or move
 * settles, the current size and position are written into the slots of
 * whatever mode is active, so the next launch (and the next switch back)
 * restores the window exactly where the user left it. The backend skips
 * maximized/fullscreen states on its own.
 */
export const useModeWindowSize = () => {
  const { simpleMode } = useSimpleMode()

  // The listener lives for the whole session; the ref keeps it reading the
  // current mode without resubscribing on every switch.
  const simpleModeRef = useRef(simpleMode)
  simpleModeRef.current = simpleMode

  useEffect(() => {
    const appWindow = getCurrentWebviewWindow()
    let timer: ReturnType<typeof setTimeout> | undefined

    const scheduleSave = () => {
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        saveWindowSizeForMode(simpleModeRef.current).catch(() => {})
      }, SAVE_DEBOUNCE_MS)
    }

    const unlistenPromises = [
      appWindow.onResized(scheduleSave),
      appWindow.onMoved(scheduleSave),
    ]

    return () => {
      if (timer) clearTimeout(timer)
      for (const promise of unlistenPromises) {
        promise.then((unlisten) => unlisten()).catch(() => {})
      }
    }
  }, [])
}
