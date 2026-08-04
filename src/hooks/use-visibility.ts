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
/** Переспросить окно; есть, только пока слушатели заведены. */
let recheck: (() => void) | undefined

const set = (next: boolean) => {
  if (next === visible) return
  visible = next
  listeners.forEach((listener) => listener())
}

/**
 * Сторож против залипания в «не видно».
 *
 * Ответ хранится один на всё приложение, и ошибиться он может в две стороны.
 * «Видно», когда на деле не видно, — это ровно то, как приложение работало
 * раньше: лишние опросы, не более. А вот застрять в «не видно» значит
 * заморозить экран целиком, поэтому именно этот случай и лечим: пока ответ
 * отрицательный, раз в минуту переспрашиваем окно. В трее это одно
 * пробуждение в минуту вместо двадцати, которые там были до правки, а зависеть
 * от одних только событий не хочется: `document.hidden` под Tauri врёт
 * (tauri-apps/tauri#10592), и события окна на разных платформах приходят
 * по-разному.
 */
const WATCHDOG_MS = 60_000

const start = () => {
  const appWindow = getCurrentWindow()
  let stopped = false
  let timer: ReturnType<typeof setTimeout> | null = null
  let watchdog: ReturnType<typeof setInterval> | null = null

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

  // Пользователь щёлкнул или нажал клавишу — окно перед ним, что бы там ни
  // отвечали события. Самая дешёвая страховка из всех.
  const shown = () => set(true)

  document.addEventListener('focus', checkSoon)
  document.addEventListener('pointerdown', shown)
  document.addEventListener('keydown', shown)
  document.addEventListener('visibilitychange', checkSoon)
  window.addEventListener('focus', checkSoon)

  watchdog = setInterval(() => {
    if (!visible) void check()
  }, WATCHDOG_MS)

  recheck = checkSoon

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
    recheck = undefined
    if (timer) clearTimeout(timer)
    if (watchdog) clearInterval(watchdog)
    document.removeEventListener('focus', checkSoon)
    document.removeEventListener('pointerdown', shown)
    document.removeEventListener('keydown', shown)
    document.removeEventListener('visibilitychange', checkSoon)
    window.removeEventListener('focus', checkSoon)
    void unlistenFocusChanged.then((unlisten) => unlisten())
    void unlistenCloseRequested.then((unlisten) => unlisten())
  }
}

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  // Каждый новый подписчик — повод переспросить: экран, который только что
  // смонтировался, видит окно живым, чем бы ни закончился прошлый ответ.
  recheck?.()
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
