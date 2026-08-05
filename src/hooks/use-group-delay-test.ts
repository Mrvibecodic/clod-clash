import { useCallback } from 'react'
import { delayGroup } from 'tauri-plugin-mihomo-api'

import { useGroupTestUrls } from '@/hooks/use-group-test-urls'
import { useAppRefreshers } from '@/providers/app-data-context'
import { restoreSelectedNodes } from '@/services/cmds'

/**
 * clod: групповой тест задержек, который не трогает выбранный сервер.
 *
 * Обработчик `/group/{name}/delay` в mihomo сбрасывает закреплённый узел
 * url-test/fallback групп (`ForceSet("")`) и затирает его в store-selected
 * кэше ядра — из-за этого «тест серверов» ронял выбор пользователя. Здесь
 * тест идёт с `keepFixed` (плагин возвращает закреплённый узел на место),
 * а затем бэкенд дополнительно сверяет выбор с сохранённым в профиле.
 *
 * Measuring is all it does. Walking off a dead node is the core's job: the
 * failover that used to live here rewrote the user's pinned choice in the
 * profile itself, silently and on every app start — this test runs
 * automatically, not only behind the "Test" button.
 */
export const useGroupDelayTest = () => {
  const { refreshProxy } = useAppRefreshers()
  const { urlFor } = useGroupTestUrls()

  // clod: the handler has to stay stable — the home row keeps it in the deps
  // of the effect that fires the delayed auto-test, and a new identity on
  // every render would reset the 800ms timer before it ever counted down.
  return useCallback(
    async (groupName: string) => {
      try {
        // clod: у каждой группы шаблона свой `url:` — тестируем YouTube-группу
        // по YouTube, а не по общему generate_204. Адрес спрашиваем у
        // delayManager, а не записываем в него: по нему же он потом читает
        // историю замеров (`extra[url]`), и разойтись они не должны.
        const testUrl = urlFor(groupName)
        await delayGroup(
          groupName,
          testUrl,
          10000,
          true, // keepFixed — вернуть закреплённый узел после теста
        )
        // Сверка с сохранённым выбором — страховка на случай, когда ядро
        // успело потерять закрепление (перезагрузка конфига во время теста).
        await restoreSelectedNodes().catch(() => {})
      } finally {
        refreshProxy().catch(() => {})
      }
    },
    [refreshProxy, urlFor],
  )
}
