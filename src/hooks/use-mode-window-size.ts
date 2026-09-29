import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useEffect, useRef } from 'react'

import { useSimpleMode } from '@/hooks/use-simple-mode'
import { applyWindowSizeForMode, saveWindowSizeForMode } from '@/services/cmds'

const SAVE_DEBOUNCE_MS = 800

// Размер и место окна запоминаются для каждого режима. Выключать подгон
// окна под содержимое здесь больше не нужно: это делает признак ручного
// изменения высоты от системы (manual_resize.rs), а на Linux — ручки рамки.
export const useModeWindowSize = () => {
  const { simpleMode } = useSimpleMode()

  const simpleModeRef = useRef(simpleMode)
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  )

  useEffect(() => {
    const appWindow = getCurrentWebviewWindow()

    const scheduleSave = () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
      saveTimerRef.current = setTimeout(() => {
        saveTimerRef.current = undefined
        saveWindowSizeForMode(simpleModeRef.current).catch(() => {})
      }, SAVE_DEBOUNCE_MS)
    }

    const unlistenPromises = [
      appWindow.onResized(scheduleSave),
      appWindow.onMoved(scheduleSave),
    ]

    return () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
      for (const promise of unlistenPromises) {
        promise.then((unlisten) => unlisten()).catch(() => {})
      }
    }
  }, [])

  useEffect(() => {
    const previous = simpleModeRef.current
    simpleModeRef.current = simpleMode
    if (previous === simpleMode) return

    const run = async () => {
      if (saveTimerRef.current) {
        clearTimeout(saveTimerRef.current)
        saveTimerRef.current = undefined
      }
      await saveWindowSizeForMode(previous).catch(() => {})
      await applyWindowSizeForMode(simpleMode).catch(() => {})
    }

    void run()
  }, [simpleMode])
}
