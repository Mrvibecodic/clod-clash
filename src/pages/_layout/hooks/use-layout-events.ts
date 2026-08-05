import { useEffect } from 'react'

import { useListen } from '@/hooks/use-listen'
import { revalidateQueries } from '@/services/query-client'

export const useLayoutEvents = (
  handleNotice: (payload: [string, string]) => void,
) => {
  const { addListener } = useListen()

  useEffect(() => {
    const unlisteners: Array<() => void> = []
    let disposed = false
    const revalidateKeys = (keys: readonly string[]) => {
      void revalidateQueries(keys.map((key) => [key]))
    }

    const register = (
      maybeUnlisten: void | (() => void) | Promise<void | (() => void)>,
    ) => {
      if (!maybeUnlisten) return

      if (typeof maybeUnlisten === 'function') {
        unlisteners.push(maybeUnlisten)
        return
      }

      maybeUnlisten
        .then((unlisten) => {
          if (!unlisten) return
          if (disposed) {
            unlisten()
          } else {
            unlisteners.push(unlisten)
          }
        })
        .catch((error) =>
          console.error('[Event Listener] Registration failed:', error),
        )
    }

    register(
      addListener('verge://refresh-clash-config', () => {
        revalidateKeys([
          'getProxies',
          'getVersion',
          'getClashConfig',
          'getRuntimeConfig',
          'getProxyProviders',
          'getRules',
          'getRuleProviders',
        ])
      }),
    )

    register(
      addListener('verge://refresh-verge-config', () => {
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
      }),
    )

    register(
      addListener('verge://notice-message', ({ payload }) =>
        handleNotice(payload as [string, string]),
      ),
    )

    return () => {
      disposed = true
      const errors: Error[] = []

      unlisteners.forEach((unlisten) => {
        try {
          unlisten()
        } catch (error) {
          errors.push(error instanceof Error ? error : new Error(String(error)))
        }
      })

      if (errors.length > 0) {
        console.error(
          `[Event Listener] Encountered ${errors.length} errors during cleanup:`,
          errors,
        )
      }

      unlisteners.length = 0
    }
  }, [addListener, handleNotice])
}
