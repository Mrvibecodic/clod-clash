import useSWR from 'swr'

import { getFreezeMarks } from '@/services/cmds'

/** Stable identity for "nothing is marked" — the common case. */
const EMPTY: Record<string, FreezeMark> = {}

/**
 * clod:freeze — пометки «режется» / «не отвечает» по имени узла для текущей
 * подписки в текущей сети. Бэкенд пересчитывает их после каждого захода
 * проверки и шлёт `clod://freeze-marks`; на смену подписки и перезапуск ядра
 * отвечает `verge://refresh-clash-config` (ключ в списке префиксов там).
 *
 * Пустая карта — норма: ничего не помечено, панель не включила проверку
 * (`clod-16-20-check`) или ядро чужое (без отпечатков узлов).
 */
export const useFreezeMarks = () => {
  const { data } = useSWR('freezeMarks', getFreezeMarks, {
    revalidateOnFocus: false,
  })

  return data ?? EMPTY
}
