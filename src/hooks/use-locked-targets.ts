import { useEffect, useRef } from 'react'

import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'

/**
 * clod:connect-mode — провайдер назвал способ подключения (`clod-connect-mode`
 * на запертом профиле), значит цель, которую он НЕ назвал, из приложения
 * больше не управляется: кнопка Connect её не трогает, переключателей у
 * пользователя нет. Оставленная включённой, она висит вечно — ровно это и
 * увидел тестировщик: заголовок «только TUN», а системный прокси Windows
 * включён и показывает 127.0.0.1:7897, пока клиент пишет «Не подключено».
 *
 * Поэтому лишнюю цель гасим один раз при появлении замка. Системный прокси
 * определяется по факту нашего адреса в системе (`indicator`) и по флагу в
 * конфиге: без второго слагаемого прокси, ещё не поданный в ОС, пережил бы
 * проверку и поднялся бы следом.
 *
 * Гасим ОДИН раз на каждый набор целей: повтор ничего не исправит (если
 * выключение не сработало, оно не сработает и на второй итерации), а
 * бесконечный цикл «эффект → патч → перерисовка → эффект» устроит легко.
 */
export const useEnforceLockedTargets = () => {
  const { targetSys, targetTun, targetsLocked } = useConnectTargets()
  const {
    indicator: sysproxyOn,
    configState: sysproxyWanted,
    toggleSystemProxy,
  } = useSystemProxyState()
  const { tunActive, tunDesired, mutateTunState } = useTunState()
  const { patchVerge } = useVerge()

  const handledRef = useRef<string | undefined>(undefined)

  useEffect(() => {
    if (!targetsLocked) {
      handledRef.current = undefined
      return
    }

    const key = `${targetSys}:${targetTun}`
    if (handledRef.current === key) return

    const strandedSys = !targetSys && (sysproxyOn || sysproxyWanted)
    const strandedTun = !targetTun && (tunActive || tunDesired)
    if (!strandedSys && !strandedTun) return

    handledRef.current = key
    void (async () => {
      try {
        if (strandedSys) {
          await toggleSystemProxy(false)
        }
        if (strandedTun) {
          await patchVerge({ enable_tun_mode: false })
          await mutateTunState()
        }
      } catch {
        // Уборка — вторичное действие: провалилась, значит цель так и осталась
        // поднятой, и об этом честнее промолчать, чем показывать ошибку,
        // которой пользователь не вызывал.
      }
    })()
  }, [
    targetsLocked,
    targetSys,
    targetTun,
    sysproxyOn,
    sysproxyWanted,
    tunActive,
    tunDesired,
    toggleSystemProxy,
    patchVerge,
    mutateTunState,
  ])
}
