import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useEffect, useRef } from 'react'

import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useVerge } from '@/hooks/use-verge'
import {
  isSelfWindowResize,
  markSelfWindowResize,
  resumeWindowFit,
  suspendWindowFit,
} from '@/hooks/use-window-fit'
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
  const { verge, patchVerge } = useVerge()

  // The listener lives for the whole session; the ref keeps it reading the
  // current mode without resubscribing on every switch.
  const simpleModeRef = useRef(simpleMode)
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(
    undefined,
  )
  // clod:fit-window — тот же приём для настройки автоподгона: слушатель
  // подписывается один раз, а читать должен свежее значение.
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
    // Масштаб экрана нужен, чтобы перевести физический размер из события в
    // логический и сверить его с тем, что просил автоподгон. Спрашиваем один
    // раз и обновляем по событию: на каждый кадр перетаскивания края ходить в
    // бэкенд незачем.
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

    // clod:fit-window — главный в размерах пользователь: потянул окно за край
    // — автоподгон выключается, и дальше живёт заданный им размер (прокрутка
    // в нём законна). Развернуть окно на весь экран — не выбор размера, а
    // временное состояние: и разворот, и возврат из него пропускаем.
    const onResized = async (height: number) => {
      const maximized = await appWindow.isMaximized().catch(() => false)
      const transient = maximized || wasMaximized
      wasMaximized = maximized
      if (transient) return

      if (!isSelfWindowResize(height) && fitEnabledRef.current) {
        suspendWindowFit()
        // Не смогли записать настройку — не притворяемся, что выключили:
        // иначе автоподгон молча не работал бы до перезапуска.
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
      // clod:fit-window — смена режима двигает окно сама, и это НЕ ручной
      // ресайз: без пометки автоподгон выключился бы от собственного щелчка
      // по кнопке «Расширенный режим».
      markSelfWindowResize()
      await applyWindowSizeForMode(simpleMode).catch(() => {})
    }

    void run()
  }, [simpleMode])
}
