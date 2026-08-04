import { TauriEvent } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useSyncExternalStore } from 'react'

const isDocumentVisible = () =>
  typeof document === 'undefined' || document.visibilityState === 'visible'

/**
 * clod: видно ли окно — один ответ на всё приложение.
 *
 * Хук зовут больше десятка мест (опросы бэкенда, таймеры, графики), и у
 * каждого раньше был свой набор слушателей плюс собственный вызов
 * `isVisible()` по IPC на каждое событие фокуса. Слушатели одинаковые и ответ
 * одинаковый — значит, и подписка нужна одна: состояние живёт в модуле,
 * компоненты читают его через `useSyncExternalStore`, слушатели заводятся на
 * первом подписчике и снимаются с последним.
 *
 * Одного `document.hidden` мало: окно уезжает в трей целиком, а документ
 * продолжает считать себя видимым — поэтому ответ сверяется с окном Tauri.
 */
let visible = isDocumentVisible()
const listeners = new Set<() => void>()
let stop: (() => void) | undefined

const set = (next: boolean) => {
  if (next === visible) return
  visible = next
  listeners.forEach((listener) => listener())
}

const start = () => {
  const appWindow = getCurrentWindow()
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | null = null

  const check = async () => {
    const windowVisible = await appWindow.isVisible().catch(() => true)
    if (!stopped) set(isDocumentVisible() && windowVisible)
  }

  // На одно изменение прилетает несколько событий (focus, visibilitychange,
  // ответ окна) — склеиваем их в один вопрос к бэкенду.
  const checkSoon = () => {
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => {
      timer = null
      void check()
    }, 50)
  }

  const shown = () => set(true)

  document.addEventListener('focus', checkSoon)
  document.addEventListener('pointerdown', shown)
  document.addEventListener('visibilitychange', checkSoon)
  window.addEventListener('focus', checkSoon)

  const unlistenFocusChanged = appWindow.onFocusChanged(checkSoon)
  const unlistenCloseRequested = appWindow.listen(
    TauriEvent.WINDOW_CLOSE_REQUESTED,
    () => {
      // Закрытие окна — это уход в трей, и знать об этом надо сразу: опрос,
      // который проснётся секундой позже, уже никому не нужен.
      set(false)
      checkSoon()
    },
  )
  void check()

  return () => {
    stopped = true
    if (timer) clearTimeout(timer)
    document.removeEventListener('focus', checkSoon)
    document.removeEventListener('pointerdown', shown)
    document.removeEventListener('visibilitychange', checkSoon)
    window.removeEventListener('focus', checkSoon)
    void unlistenFocusChanged.then((unlisten) => unlisten())
    void unlistenCloseRequested.then((unlisten) => unlisten())
  }
}

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  if (listeners.size === 1) {
    try {
      stop = start()
    } catch {
      // Вне Tauri (`pnpm web:dev`) окна нет и `getCurrentWindow` бросает.
      // Бросить дальше значило бы оставить стор с подписчиком, но без
      // слушателей: следующий подписчик увидел бы size > 1 и не запустил бы их
      // уже никогда. Пусть в вебе видимость просто всегда «да».
      stop = undefined
    }
  }

  return () => {
    listeners.delete(listener)
    if (listeners.size === 0) {
      stop?.()
      stop = undefined
    }
  }
}

const snapshot = () => visible

export const useVisibility = () =>
  useSyncExternalStore(subscribe, snapshot, snapshot)
