import { useEffect, useRef } from 'react'

import { useVisibility } from '@/hooks/use-visibility'

/**
 * clod: вернулись из трея — спрашиваем данные сразу, а не ждём следующего тика.
 *
 * Опросы у нас привязаны к видимости окна (`refetchInterval: visible ? … :
 * false`), и это правильно: за окном в трее опрашивать нечего. Но возобновление
 * интервала запрос НЕ делает — SWR просто заводит таймер заново. Пользователь,
 * развернувший окно, до первого тика видит цифры прошлого показа: десять секунд
 * для TUN, тридцать для режима работы, а на главной — вообще сколько угодно,
 * там опроса нет совсем. Именно это и читается как «зависшие данные».
 *
 * Хук возвращает ту же видимость, что и `useVisibility`, чтобы вызывающий не
 * подписывался на неё дважды: `const visible = useRefreshOnReturn(refetch)`.
 */
export const useRefreshOnReturn = (
  refresh: () => unknown | Promise<unknown>,
) => {
  const visible = useVisibility()

  // Через ref: `refetch` у SWR — новая функция на каждый рендер, и в
  // зависимостях эффекта она превратила бы «спросить один раз» в опрос без
  // остановки.
  const refreshRef = useRef(refresh)
  useEffect(() => {
    refreshRef.current = refresh
  })

  // Строго на переходе «не видно → видно»: первый запрос на маунте делает сам
  // `useQuery`, дублировать его при каждом открытии экрана незачем.
  const wasVisibleRef = useRef(visible)
  useEffect(() => {
    const returned = visible && !wasVisibleRef.current
    wasVisibleRef.current = visible
    if (returned) void refreshRef.current()
  }, [visible])

  return visible
}
