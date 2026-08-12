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
 * Сторож против залипания — в обе стороны.
 *
 * Ответ хранится один на всё приложение, и ошибиться он может двояко.
 * Застрять в «не видно» значит заморозить экран целиком; застрять в «видно»
 * при окне в трее — вернуть ровно те опросы, ради которых всё это писалось.
 * Полагаться на одни события нельзя: `document.hidden` под Tauri врёт
 * (tauri-apps/tauri#10592), события окна на разных платформах приходят
 * по-разному, а в «видно» стор попадает ещё и по клику с клавишей, без всякой
 * проверки. Поэтому раз в минуту переспрашиваем окно безусловно — это один
 * запрос против двадцати опросов ядра, которые тут были до правки.
 */
const WATCHDOG_MS = 60_000

const start = () => {
  // Вне Tauri (`pnpm web:dev`) окна нет; тогда слушателей не заводим вовсе и
  // видимость остаётся «да». Бросить отсюда нельзя: подписчик уже в списке, и
  // следующий увидел бы size > 1 и не запустил бы слушателей уже никогда.
  let appWindow: ReturnType<typeof getCurrentWindow>
  try {
    appWindow = getCurrentWindow()
  } catch {
    return undefined
  }

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

  watchdog = setInterval(() => void check(), WATCHDOG_MS)

  recheck = checkSoon

  // clod:window-return — момент показа бэкенд знает точно и говорит о нём сам
  // («verge://window-shown» из `WindowManager`). Отвечаем без переспроса:
  // `is_visible()` для только что развёрнутого окна на Windows успевает
  // соврать, а единственной безусловной проверкой оставался сторож раз в
  // минуту — до неё экран показывал цифры прошлого показа.
  const unlistenShown = appWindow.listen('verge://window-shown', shown)

  const unlistenFocusChanged = appWindow.onFocusChanged(checkSoon)
  const unlistenCloseRequested = appWindow.listen(
    TauriEvent.WINDOW_CLOSE_REQUESTED,
    () => {
      // Закрытие окна — это уход в трей, и знать об этом надо сразу: опрос,
      // который проснётся секундой позже, уже никому не нужен. Переспрашивать
      // следом не надо — окно ещё может ответить «видно» (прячет его бэкенд, и
      // не мгновенно), а событие о потере фокуса всё равно придёт.
      set(false)
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
    void unlistenShown.then((unlisten) => unlisten())
    void unlistenFocusChanged.then((unlisten) => unlisten())
    void unlistenCloseRequested.then((unlisten) => unlisten())
  }
}

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  // Каждый новый подписчик — повод переспросить: экран, который только что
  // смонтировался, видит окно живым, чем бы ни закончился прошлый ответ.
  recheck?.()
  if (listeners.size === 1) stop = start()

  return () => {
    listeners.delete(listener)
    if (listeners.size === 0) {
      stop?.()
      stop = undefined
    }
  }
}

const snapshot = () => visible
/** Рендера на сервере тут нет; отвечаем «видно», как и при первом запуске. */
const serverSnapshot = () => true

export const useVisibility = () =>
  useSyncExternalStore(subscribe, snapshot, serverSnapshot)
