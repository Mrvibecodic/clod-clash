import { useEffect, useRef } from 'react'

import { useTauriEvent } from '@/hooks/use-listen'
import { useVisibility } from '@/hooks/use-visibility'
import { revalidateQueries } from '@/services/query-client'

const revalidateKeys = (keys: readonly string[]) =>
  revalidateQueries(keys.map((key) => [key]))

const CLASH_CONFIG_KEYS_ALWAYS = ['getProxies', 'getRuntimeConfig'] as const

const CLASH_CONFIG_KEYS_WHEN_VISIBLE = [
  'getVersion',
  'getClashConfig',
  'getRules',
  'getRuleProviders',
  // clod:port-ladder — порт ядра и признак «как в подписке» живут в своих
  // запросах, и их не перечитывал никто: диалог портов после сохранения
  // показывал старое закрепление, повторное «ОК» молча возвращало его,
  // а страница настроек после смены профиля держала прежний порт.
  'getCoreLadder',
  'getClashInfo',
] as const

export const useLayoutEvents = (
  handleNotice: (payload: [string, string]) => void,
) => {
  const visible = useVisibility()
  const visibleRef = useRef(visible)
  const pendingRef = useRef(false)

  useEffect(() => {
    const returned = visible && !visibleRef.current
    visibleRef.current = visible
    if (returned && pendingRef.current) {
      pendingRef.current = false
      void revalidateKeys(CLASH_CONFIG_KEYS_WHEN_VISIBLE)
    }
  }, [visible])

  useTauriEvent('verge://refresh-clash-config', () => {
    void revalidateKeys(['getProxyProviders'])
      .catch(() => undefined)
      .then(() => revalidateKeys(CLASH_CONFIG_KEYS_ALWAYS))
    if (visibleRef.current) {
      void revalidateKeys(CLASH_CONFIG_KEYS_WHEN_VISIBLE)
    } else {
      pendingRef.current = true
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
}
