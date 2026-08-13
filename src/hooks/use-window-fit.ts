import { useCallback, useEffect, useRef, useState } from 'react'

import { useVerge } from '@/hooks/use-verge'
import { fitWindowToContent } from '@/services/cmds'

/**
 * clod:fit-window — окно садится по содержимому, чтобы прокрутки на главном
 * экране не было вовсе.
 *
 * Фиксированный размер окна эту задачу не решает: содержимое приходит от
 * панели и меняется от подписки к подписке — баннеры, строка режима, плитки.
 * Поэтому высота считается от фактической высоты контента и пересчитывается на
 * каждое его изменение, а потолком служит рабочая область экрана (её знает
 * бэкенд и возвращает нам).
 *
 * Главный в размерах — пользователь: первое же ручное изменение размера гасит
 * автоподгон (см. `use-mode-window-size.ts`), и дальше живёт заданный им
 * размер, в котором прокрутка законна. Вернуть автомат можно тумблером в
 * настройках.
 */

/** Пересчёт не чаще, чем раз в этот срок: перерисовка идёт пачками. */
const FIT_DEBOUNCE_MS = 120

/** Минимальная высота окна — тот же предел держит и бэкенд (`MINIMAL_HEIGHT`). */
const MINIMAL_HEIGHT = 520

/**
 * Запас, с которым компактная вёрстка снимается обратно. Без него содержимое,
 * стоящее ровно на границе, мигало бы плотной и обычной раскладкой.
 */
const COMPACT_HYSTERESIS = 24

/**
 * Сколько после нашего `set_size` ждать ответное событие `onResized`: оно
 * приходит из ОС с задержкой.
 */
const SELF_RESIZE_GRACE_MS = 900

/** Допуск при сверке высот: физические пиксели переводятся в логические делением. */
const HEIGHT_MATCH_EPSILON = 3

let selfResizeUntil = 0
let acceptAnyUntil = 0
/** Высоты, которые мы сами только что запросили (логические пиксели). */
const expectedHeights: number[] = []

/**
 * Пометить ближайшее событие изменения размера как своё.
 *
 * Одного времени мало: пользователь может тянуть край окна ровно в тот момент,
 * когда автоподгон отправил свой `set_size`, — и его ресайз был бы съеден как
 * наш. Поэтому вместе со сроком запоминаем и ЗАПРОШЕННУЮ высоту: событие
 * считается нашим, только если окно приехало именно туда, куда мы просили.
 * Без высоты (смена режима двигает окно целиком, размер считает бэкенд)
 * пропускаем всё, что придёт в течение срока.
 */
export const markSelfWindowResize = (height?: number) => {
  selfResizeUntil = Date.now() + SELF_RESIZE_GRACE_MS
  if (height === undefined) {
    acceptAnyUntil = selfResizeUntil
    return
  }
  expectedHeights.push(Math.round(height))
  if (expectedHeights.length > 4) expectedHeights.shift()
}

/** Событие изменения размера пришло от нас, а не от пользователя? */
export const isSelfWindowResize = (height: number) => {
  const now = Date.now()
  if (now < acceptAnyUntil) return true
  if (now >= selfResizeUntil) return false
  return expectedHeights.some(
    (expected) => Math.abs(expected - height) <= HEIGHT_MATCH_EPSILON,
  )
}

/**
 * Пользователь взял размеры на себя. Флаг синхронный и живёт рядом с
 * настройкой: запись в конфиг доедет до `useVerge` только через круг, а
 * подгонять окно нельзя уже со следующего кадра — иначе автомат будет драться
 * с рукой, тянущей край окна.
 */
let fitSuspended = false

export const suspendWindowFit = () => {
  fitSuspended = true
}

export const resumeWindowFit = () => {
  fitSuspended = false
}

/**
 * Хук главных экранов: даёт ref на прокручиваемый корень страницы и флаг
 * компактной вёрстки.
 *
 * Ref вешается на элемент с `overflowY: auto` — его `scrollHeight` и есть
 * полная высота содержимого, даже когда оно не влезло. Наблюдаем и сам корень,
 * и его прямых детей: у корня высота прибита к окну и никогда не меняется,
 * меняются именно дети.
 */
export const useFitWindowToContent = () => {
  const { verge } = useVerge()
  const enabled = verge?.window_fit_content !== false

  const [root, setRoot] = useState<HTMLElement | null>(null)
  const [compact, setCompact] = useState(false)

  const compactRef = useRef(false)
  /** Последняя высота содержимого, измеренная в обычной вёрстке. */
  const normalHeightRef = useRef(0)
  /** Насколько компактная вёрстка ниже обычной — знаем после первого перехода. */
  const compactSavingRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  const applyFit = useCallback(async () => {
    if (!enabled || fitSuspended) {
      // Размеры перешли к пользователю — возвращаем обычные отступы: плотная
      // раскладка была частью автоподгона, а не самостоятельной настройкой.
      if (compactRef.current) {
        compactRef.current = false
        setCompact(false)
      }
      return
    }
    if (!root) return

    // Хром окна: заголовок и отступы над областью страницы. Считается по
    // разнице, а не константой, — на Windows и Linux у окна ещё своя полоса.
    const chrome = Math.max(0, window.innerHeight - root.clientHeight)
    const desired = root.scrollHeight + chrome
    if (desired <= 0) return

    markSelfWindowResize(desired)
    const ceiling = await fitWindowToContent(desired).catch(() => 0)
    if (!ceiling) return
    // Бэкенд обрежет запрошенное потолком и минимумом окна — ждём именно ту
    // высоту, которая в итоге применится, иначе своё же событие не узнаем.
    markSelfWindowResize(Math.min(Math.max(desired, MINIMAL_HEIGHT), ceiling))

    if (compactRef.current) {
      // Разницу между раскладками узнаём ровно один раз — на первом же замере
      // после включения компакта, пока обычная высота ещё свежая.
      if (normalHeightRef.current > desired) {
        compactSavingRef.current = normalHeightRef.current - desired
      }
      const normalGuess = desired + compactSavingRef.current
      if (normalGuess + COMPACT_HYSTERESIS <= ceiling) {
        compactRef.current = false
        setCompact(false)
      }
      return
    }

    normalHeightRef.current = desired
    // Не влезло в рабочую область — последняя попытка обойтись без прокрутки:
    // поджать отступы и померить заново.
    if (desired > ceiling + 1) {
      compactRef.current = true
      setCompact(true)
    }
  }, [root, enabled])

  const schedule = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current)
    timerRef.current = setTimeout(() => {
      timerRef.current = undefined
      void applyFit()
    }, FIT_DEBOUNCE_MS)
  }, [applyFit])

  // Тумблер вернули во включённое положение — автомат снова главный.
  useEffect(() => {
    if (enabled) resumeWindowFit()
  }, [enabled])

  useEffect(() => {
    if (!root) return

    const observer = new ResizeObserver(schedule)
    const observeAll = () => {
      observer.disconnect()
      observer.observe(root)
      for (const child of Array.from(root.children)) observer.observe(child)
    }
    observeAll()

    // Появился баннер или исчезла карточка — у корня меняется состав детей,
    // и новых надо взять под наблюдение.
    const mutations = new MutationObserver(() => {
      observeAll()
      schedule()
    })
    mutations.observe(root, { childList: true })

    schedule()

    return () => {
      observer.disconnect()
      mutations.disconnect()
      if (timerRef.current) {
        clearTimeout(timerRef.current)
        timerRef.current = undefined
      }
    }
  }, [root, schedule])

  return { fitRef: setRoot, compact }
}
