import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useEffect, useRef } from 'react'

import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useVerge } from '@/hooks/use-verge'
import {
  isSelfWindowResize,
  isStartupWindowGrace,
  markSelfWindowResize,
  markStartupWindowSettled,
  resumeWindowFit,
  suspendWindowFit,
} from '@/hooks/use-window-fit'
import { applyWindowSizeForMode, saveWindowSizeForMode } from '@/services/cmds'

const SAVE_DEBOUNCE_MS = 800

export const useModeWindowSize = () => {
  const { simpleMode } = useSimpleMode()
  const { verge, patchVerge } = useVerge()

  const simpleModeRef = useRef(simpleMode)
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  )
  const fitEnabled = verge?.window_fit_content !== false
  const fitEnabledRef = useRef(fitEnabled)
  const patchVergeRef = useRef(patchVerge)

  useEffect(() => {
    fitEnabledRef.current = fitEnabled
    patchVergeRef.current = patchVerge
  })

  useEffect(() => {
    const appWindow = getCurrentWebviewWindow()
    let wasMaximized = false
    let wasMinimized = false
    let scale = 1
    void appWindow
      .scaleFactor()
      .then((value) => {
        scale = value
      })
      .catch(() => {})

    const scheduleSave = () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
      saveTimerRef.current = setTimeout(() => {
        saveTimerRef.current = undefined
        saveWindowSizeForMode(simpleModeRef.current).catch(() => {})
      }, SAVE_DEBOUNCE_MS)
    }

    const onResized = async (height: number) => {
      if (!Number.isFinite(height) || height <= 0) return

      const [maximized, minimized] = await Promise.all([
        appWindow.isMaximized().catch(() => false),
        appWindow.isMinimized().catch(() => false),
      ])
      const transient = maximized || wasMaximized || minimized || wasMinimized
      wasMaximized = maximized
      wasMinimized = minimized
      if (transient) return

      if (isSelfWindowResize(height)) {
        markStartupWindowSettled()
      } else if (!isStartupWindowGrace() && fitEnabledRef.current) {
        suspendWindowFit()
        patchVergeRef
          .current({ window_fit_content: false })
          .catch(() => resumeWindowFit())
      }
      scheduleSave()
    }

    const unlistenPromises = [
      appWindow.onResized(
        (event) => void onResized(event.payload.height / (scale || 1)),
      ),
      appWindow.onMoved(scheduleSave),
      appWindow.onScaleChanged((event) => {
        scale = event.payload.scaleFactor
        markSelfWindowResize()
      }),
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
      markSelfWindowResize()
      await applyWindowSizeForMode(simpleMode).catch(() => {})
    }

    void run()
  }, [simpleMode])
}
