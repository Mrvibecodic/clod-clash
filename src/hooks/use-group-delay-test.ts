import { useCallback } from 'react'
import { delayGroup } from 'tauri-plugin-mihomo-api'

import { useGroupTestUrls } from '@/hooks/use-group-test-urls'
import { useAppRefreshers } from '@/providers/app-data-context'

/**
 * clod: групповой тест задержек, который не трогает выбранный сервер.
 *
 * Обработчик `/group/{name}/delay` в mihomo на время теста сбрасывает
 * закреплённый узел url-test/fallback групп (`ForceSet("")`) и затирает его в
 * `store-selected` кэше ядра — из-за этого «тест серверов» ронял выбор
 * пользователя. Спасает единственное: `keepFixed`, по которому плагин
 * возвращает закреплённый узел на место после замеров.
 *
 * Раньше следом звался ещё и бэкендный `restoreSelectedNodes` — «страховка на
 * случай, когда ядро потеряло закрепление». Страховкой он не был: ядро снимает
 * закрепление только у групп url-test/fallback, а наш возврат выбора умеет
 * активировать только группы select. Пересечение пусто, то есть круг был
 * гарантированно холостым — восемь-девять запросов к ядру и семь перевалидаций
 * интерфейса на каждый автотест, включая правила, к задержкам отношения не
 * имеющие. Убран; настоящую работу делает `keepFixed`, и его отказ теперь
 * виден, а не проглатывается.
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
      } finally {
        refreshProxy().catch(() => {})
      }
    },
    [refreshProxy, urlFor],
  )
}
