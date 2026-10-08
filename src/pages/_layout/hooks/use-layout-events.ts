import { useEffect, useRef } from 'react'
import { useLocation } from 'react-router'

import { useTauriEvent } from '@/hooks/use-listen'
import { useSubscriptionUpdateEvents } from '@/hooks/use-subscription-update'
import { useVisibility } from '@/hooks/use-visibility'
import {
  removeCacheData,
  revalidateQueries,
  revalidateQueriesByPrefix,
} from '@/services/query-client'

const revalidateKeys = (keys: readonly string[]) =>
  revalidateQueries(keys.map((key) => [key]))

const CLASH_CONFIG_KEYS_ALWAYS = ['getProxies', 'getRuntimeConfig'] as const

const CLASH_CONFIG_KEYS_WHEN_VISIBLE = [
  'getVersion',
  'getClashConfig',
  // clod:port-ladder — порт ядра и признак «как в подписке» живут в своих
  // запросах, и их не перечитывал никто: диалог портов после сохранения
  // показывал старое закрепление, повторное «ОК» молча возвращало его,
  // а страница настроек после смены профиля держала прежний порт.
  'getCoreLadder',
  'getClashInfo',
] as const

const CLASH_CONFIG_KEYS_ON_RETURN = CLASH_CONFIG_KEYS_WHEN_VISIBLE.filter(
  (key) => key !== 'getClashConfig',
)

/**
 * Правила ядра и их провайдеры читает один экран «Правила», и при открытии он
 * перечитывает их сам. Вне его пересборка конфига их только сбрасывает: ответ
 * бывает большим, а при открытии иначе мелькнули бы прежние.
 */
const RULES_KEYS = ['getRules', 'getRuleProviders'] as const
const RULES_PATH = '/rules'

const CLASH_CONFIG_PREFIXES = [
  'sentinelReport',
  'serverDescriptions',
  'freezeMarks',
] as const

export const useLayoutEvents = (
  handleNotice: (payload: [string, string]) => void,
) => {
  const visible = useVisibility()
  const { pathname } = useLocation()
  const visibleRef = useRef(visible)
  const pendingRef = useRef(false)

  useEffect(() => {
    const returned = visible && !visibleRef.current
    visibleRef.current = visible
    if (returned && pendingRef.current) {
      pendingRef.current = false
      void revalidateKeys(CLASH_CONFIG_KEYS_ON_RETURN)
    }
  }, [visible])

  useTauriEvent('verge://refresh-clash-config', () => {
    // Провайдеров прокси перечитывает само чтение групп, когда ядро сменилось
    void revalidateKeys(CLASH_CONFIG_KEYS_ALWAYS)
    void revalidateQueriesByPrefix(CLASH_CONFIG_PREFIXES)
    if (visibleRef.current) {
      void revalidateKeys(CLASH_CONFIG_KEYS_WHEN_VISIBLE)
    } else {
      pendingRef.current = true
    }
    if (pathname !== RULES_PATH) {
      for (const key of RULES_KEYS) void removeCacheData([key])
    } else if (visibleRef.current) {
      // В свёрнутом окне экран «Правила» перечитает их сам при показе.
      void revalidateKeys(RULES_KEYS)
    }
  })

  useTauriEvent('verge://refresh-verge-config', () => {
    revalidateKeys([
      'getVergeConfig',
      'getSystemProxy',
      'getAutotemProxy',
      'getSystemState',
      // clod:tun-ready — бэкенд шлёт это событие в том числе когда сам
      // погасил туннель. Без перечитывания состояние TUN обновлял только
      // опрос (10 с), а в свёрнутом окне — вообще никто: и тумблер, и
      // кнопка Connect до десяти секунд врали о туннеле.
      'getTunState',
    ])
  })

  useTauriEvent<[string, string]>('verge://notice-message', ({ payload }) =>
    handleNotice(payload),
  )

  // clod:freeze — пометки «режется» / «не отвечает» пересчитаны бэкендом.
  useTauriEvent('clod://freeze-marks', () => {
    void revalidateKeys(['freezeMarks'])
  })

  // clod: логотип или фон подписки на диске сменился — скачан заново или убран.
  useTauriEvent<{ uid: string; picture: 'logo' | 'background' }>(
    'clod://profile-picture',
    ({ payload }) => {
      const key =
        payload.picture === 'logo' ? 'profileLogo' : 'profileBackground'
      void revalidateQueries([[key, payload.uid]])
    },
  )

  useSubscriptionUpdateEvents()
}
