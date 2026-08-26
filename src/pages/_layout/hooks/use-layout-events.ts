import { useTauriEvent } from '@/hooks/use-listen'
import { revalidateQueries } from '@/services/query-client'

const revalidateKeys = (keys: readonly string[]) => {
  void revalidateQueries(keys.map((key) => [key]))
}

export const useLayoutEvents = (
  handleNotice: (payload: [string, string]) => void,
) => {
  useTauriEvent('verge://refresh-clash-config', () => {
    revalidateKeys([
      'getProxies',
      'getVersion',
      'getClashConfig',
      'getRuntimeConfig',
      'getProxyProviders',
      'getRules',
      'getRuleProviders',
    ])
  })

  useTauriEvent('verge://refresh-verge-config', () => {
    revalidateKeys([
      'getVergeConfig',
      'getSystemProxy',
      'getAutotemProxy',
      'getRunningMode',
      'isServiceAvailable',
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
