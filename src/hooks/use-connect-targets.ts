import { useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useSystemState } from '@/hooks/use-system-state'
import { useVerge } from '@/hooks/use-verge'

/**
 * What the Connect button actually switches.
 *
 * System proxy and TUN are independent targets which may be driven together
 * (`verge.connect_system_proxy` / `verge.connect_tun_mode`, configured in the
 * settings). The default is the system proxy alone; when the user manages to
 * disable both, the system proxy silently steps back in — a Connect button
 * that switches nothing is a broken promise.
 *
 * `connected` reads the real state of every enabled target, never an
 * optimistic flag: a system proxy dropped from outside the app must turn the
 * button dark by itself.
 */
export const useConnectTargets = () => {
  const { t } = useTranslation()
  const { verge, patchVerge } = useVerge()
  const { isTunModeAvailable } = useSystemState()
  const { indicator: sysproxyOn, toggleSystemProxy } = useSystemProxyState()

  const tunOn = Boolean(verge?.enable_tun_mode)

  const { targetSys, targetTun } = useMemo(() => {
    const sys = verge?.connect_system_proxy ?? true
    const tun = verge?.connect_tun_mode ?? false
    return { targetSys: sys || !tun, targetTun: tun }
  }, [verge?.connect_system_proxy, verge?.connect_tun_mode])

  const connected =
    (!targetSys || sysproxyOn) &&
    (!targetTun || tunOn) &&
    (targetSys || targetTun)

  const toggleConnection = useCallback(async () => {
    const next = !connected

    if (next && targetTun && !isTunModeAvailable && !tunOn) {
      throw new Error(t('home.components.connect.errors.serviceRequired'))
    }

    if (targetTun && tunOn !== next) {
      await patchVerge({ enable_tun_mode: next })
    }
    if (targetSys && sysproxyOn !== next) {
      await toggleSystemProxy(next)
    }
  }, [
    connected,
    targetSys,
    targetTun,
    tunOn,
    sysproxyOn,
    isTunModeAvailable,
    patchVerge,
    toggleSystemProxy,
    t,
  ])

  return { connected, targetSys, targetTun, toggleConnection }
}
