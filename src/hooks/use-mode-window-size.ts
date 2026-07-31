import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useEffect, useRef } from 'react'

import { useSimpleMode } from '@/hooks/use-simple-mode'
import { saveWindowSizeForMode } from '@/services/cmds'

const SAVE_DEBOUNCE_MS = 800

/**
 * Keeps the per-mode window size fresh: after a manual resize settles, the
 * current size is written into the slot of whatever mode is active, so the
 * next launch (and the next switch back) restores it. The backend skips
 * maximized/fullscreen sizes on its own.
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

    const unlistenPromise = appWindow.onResized(() => {
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        saveWindowSizeForMode(simpleModeRef.current).catch(() => {})
      }, SAVE_DEBOUNCE_MS)
    })

    return () => {
      if (timer) clearTimeout(timer)
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {})
    }
  }, [])
}
