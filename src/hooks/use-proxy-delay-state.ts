import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useReducer } from 'react'

import { useVerge } from '@/hooks/use-verge'
import delayManager, {
  type DelayUpdate,
  effectiveLatencyTimeout,
} from '@/services/delay'
import { isCorePolicy } from '@/utils/proxy-groups'

const identity = (_: DelayUpdate, next: DelayUpdate): DelayUpdate => next

const INITIAL_DELAY: DelayUpdate = { delay: -1, updatedAt: 0 }

export interface UseProxyDelayState {
  delayValue: number
  isPreset: boolean
  timeout: number
  onDelay: (providerName?: string) => Promise<void>
}

export function useProxyDelayState(
  proxy: IProxyItem,
  groupName: string,
): UseProxyDelayState {
  const isPreset = isCorePolicy(proxy.name)
  const [delayState, setDelayState] = useReducer(identity, INITIAL_DELAY)
  const { verge } = useVerge()
  const timeout = effectiveLatencyTimeout(verge?.default_latency_timeout)

  useEffect(() => {
    if (isPreset) return
    delayManager.setListener(proxy.name, groupName, setDelayState)
    return () => {
      delayManager.removeListener(proxy.name, groupName)
    }
  }, [proxy.name, groupName, isPreset])

  const updateDelay = useCallback(() => {
    setDelayState({
      delay: delayManager.getDelayFix(proxy, groupName),
      updatedAt: delayManager.getMeasuredAt(proxy, groupName),
    })
  }, [proxy, groupName])

  useEffect(() => {
    updateDelay()
  }, [updateDelay])

  const onDelay = useLockFn(async (providerName?: string) => {
    setDelayState({ delay: -2, updatedAt: Date.now() })
    setDelayState(
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
    timeout,
    onDelay,
  }
}
