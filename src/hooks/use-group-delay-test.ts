import { useCallback } from 'react'
import { delayGroup, getProxies } from 'tauri-plugin-mihomo-api'

import { useGroupTestUrls } from '@/hooks/use-group-test-urls'
import { useProfiles } from '@/hooks/use-profiles'
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useAppRefreshers } from '@/providers/app-data-context'
import { restoreSelectedNodes } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { nameWithoutFlag } from '@/utils/country'
import { SELECTABLE_GROUP_TYPES } from '@/utils/proxy-groups'

/**
 * clod: групповой тест задержек, который не трогает выбранный сервер.
 *
 * Обработчик `/group/{name}/delay` в mihomo сбрасывает закреплённый узел
 * url-test/fallback групп (`ForceSet("")`) и затирает его в store-selected
 * кэше ядра — из-за этого «тест серверов» ронял выбор пользователя. Здесь
 * тест идёт с `keepFixed` (плагин возвращает закреплённый узел на место),
 * а затем бэкенд дополнительно сверяет выбор с сохранённым в профиле.
 *
 * Вторая обязанность — автопереход на избранный: если выбранный узел не
 * ответил на тест, а среди избранных есть живой, выбор переводится на него.
 * Избранные при этом никогда не перехватывают живой выбор.
 */
export const useGroupDelayTest = () => {
  const { current } = useProfiles()
  const { refreshProxy } = useAppRefreshers()
  const { urlFor } = useGroupTestUrls()

  // clod: обработчики обязаны быть стабильными. `useProxySelection` держит их
  // в зависимостях `changeProxy`, а на нём висит вся цепочка до колбэка,
  // который вызывает отложенный автотест: инлайновые стрелки пересоздавали бы
  // её на каждом рендере, и таймер на 800 мс сбрасывался бы, не досчитав.
  const onSuccess = useCallback(() => {
    refreshProxy().catch(() => {})
  }, [refreshProxy])
  const onError = useCallback((error: unknown) => showNotice.error(error), [])
  const { changeProxy } = useProxySelection({ onSuccess, onError })

  const favorites = current?.favorites

  const failoverToFavorite = useCallback(
    async (groupName: string, delays: Record<string, number>) => {
      const starred = favorites ?? []
      if (starred.length === 0) return

      // Свежие данные ядра, не SWR-кэш: тест и restore только что закончились.
      const data = await getProxies()
      const records = (data?.proxies ?? {}) as Record<string, any>
      const group = records[groupName]
      const members: string[] = group?.all ?? []
      const type = (group?.type ?? '').toLowerCase()
      if (!SELECTABLE_GROUP_TYPES.has(type) || members.length === 0) return

      const now: string | undefined = group?.now
      if (!now) return
      // Выбор указывает на вложенную группу — там ядро само выбирает узел.
      if (records[now]?.all) return

      // Тест возвращает только ответившие узлы: отсутствие = недоступен.
      const alive = (name: string) => (delays[name] ?? 0) > 0
      if (alive(now)) return

      const candidate = starred.find(
        (name) => name !== now && members.includes(name) && alive(name),
      )
      if (!candidate) return

      changeProxy(groupName, candidate, now)
      showNotice.info('home.components.serverSelect.failover', {
        from: nameWithoutFlag(now),
        to: nameWithoutFlag(candidate),
      })
    },
    [favorites, changeProxy],
  )

  return useCallback(
    async (groupName: string) => {
      try {
        // clod: у каждой группы шаблона свой `url:` — тестируем YouTube-группу
        // по YouTube, а не по общему generate_204. Адрес спрашиваем у
        // delayManager, а не записываем в него: по нему же он потом читает
        // историю замеров (`extra[url]`), и разойтись они не должны.
        const testUrl = urlFor(groupName)
        const delays = await delayGroup(
          groupName,
          testUrl,
          10000,
          true, // keepFixed — вернуть закреплённый узел после теста
        )
        // Сверка с сохранённым выбором — страховка на случай, когда ядро
        // успело потерять закрепление (перезагрузка конфига во время теста).
        // Идёт ДО failover: иначе restore мог бы вернуть мёртвый узел поверх
        // только что выбранного избранного.
        await restoreSelectedNodes().catch(() => {})
        await failoverToFavorite(groupName, delays ?? {})
      } finally {
        refreshProxy().catch(() => {})
      }
    },
    [failoverToFavorite, refreshProxy, urlFor],
  )
}
