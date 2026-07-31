import { useCallback } from 'react'
import { delayGroup, getProxies } from 'tauri-plugin-mihomo-api'

import { useProfiles } from '@/hooks/use-profiles'
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useAppRefreshers } from '@/providers/app-data-context'
import { restoreSelectedNodes } from '@/services/cmds'
import delayManager from '@/services/delay'
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
  const { changeProxy } = useProxySelection({
    onSuccess: () => {
      refreshProxy().catch(() => {})
    },
    onError: (error) => showNotice.error(error),
  })

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
        const delays = await delayGroup(
          groupName,
          delayManager.getUrl(groupName),
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
    [failoverToFavorite, refreshProxy],
  )
}
