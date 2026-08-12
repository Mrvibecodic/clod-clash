import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useEffect, useRef } from 'react'

import { useSimpleMode } from '@/hooks/use-simple-mode'
import { applyWindowSizeForMode, saveWindowSizeForMode } from '@/services/cmds'

const SAVE_DEBOUNCE_MS = 800

/**
 * Keeps the per-mode window geometry fresh: after a manual resize or move
 * settles, the current size and position are written into the slots of
 * whatever mode is active, so the next launch (and the next switch back)
 * restores the window exactly where the user left it. The backend skips
 * maximized/fullscreen states on its own.
 *
 * clod:mode-window — и второе: окно догоняет режим, КТО БЫ его ни сменил.
 * Раньше размер применяла только кнопка «К расширенному виду», а режим,
 * пришедший заголовком `clod-simple-mode`, менял одну вёрстку: окно
 * оставалось узким, расширенный интерфейс уезжал в прокрутку вниз и понять,
 * что он вообще включился, можно было только проскроллив. Смена режима теперь
 * одна точка: сохранить геометрию покидаемого режима, применить геометрию
 * нового.
 */
export const useModeWindowSize = () => {
  const { simpleMode } = useSimpleMode()

  // The listener lives for the whole session; the ref keeps it reading the
  // current mode without resubscribing on every switch.
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
    // Первый проход пропускаем: окно уже создано под нужный режим самим
    // бэкендом (`effective_simple_mode` в момент создания), и повторное
    // применение только спорило бы с плагином window-state.
    if (previous === simpleMode) return

    const run = async () => {
      // Отложенное сохранение принадлежит ПОКИДАЕМОМУ режиму: если таймер ещё
      // тикает, он сработает уже с новым значением рефа и запишет размер не в
      // тот слот. Гасим его и сохраняем сами.
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
