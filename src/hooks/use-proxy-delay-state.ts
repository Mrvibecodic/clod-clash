import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useState } from 'react'

import { useVerge } from '@/hooks/use-verge'
import delayManager, {
  type DelayUpdate,
  effectiveLatencyTimeout,
} from '@/services/delay'
import { isCorePolicy } from '@/utils/proxy-groups'

const INITIAL_DELAY: DelayUpdate = { delay: -1, updatedAt: 0 }

export interface UseProxyDelayState {
  delayValue: number
  isPreset: boolean
  onDelay: (providerName?: string) => Promise<void>
}

export function useProxyDelayState(
  proxy: IProxyItem,
  groupName: string,
): UseProxyDelayState {
  const isPreset = isCorePolicy(proxy.name)
  const [delayState, setDelayState] = useState(INITIAL_DELAY)
  // Строка рисуется только из задержки: то же значение (метку «идёт проверка»
  // узлу ставят и пакет, и сам замер) её не перерисовывает
  const showDelay = useCallback(
    (next: DelayUpdate) =>
      setDelayState((shown) => (next.delay === shown.delay ? shown : next)),
    [],
  )
  const { verge } = useVerge()
  const timeout = effectiveLatencyTimeout(verge?.default_latency_timeout)

  useEffect(() => {
    if (isPreset) return
    delayManager.setListener(proxy.name, groupName, showDelay)
    return () => {
      delayManager.removeListener(proxy.name, groupName)
    }
  }, [proxy.name, groupName, isPreset, showDelay])

  const updateDelay = useCallback(() => {
    showDelay({
      delay: delayManager.getDelayFix(proxy, groupName),
      updatedAt: delayManager.getMeasuredAt(proxy, groupName),
    })
  }, [proxy, groupName, showDelay])

  useEffect(() => {
    updateDelay()
  }, [updateDelay])

  const onDelay = useLockFn(async (providerName?: string) => {
    showDelay({ delay: -2, updatedAt: Date.now() })
    showDelay(
      await delayManager.checkDelay(
        proxy.name,
        groupName,
        timeout,
        providerName,
      ),
    )
  })

  return {
    delayValue: delayState.delay,
    isPreset,
    onDelay,
  }
}
