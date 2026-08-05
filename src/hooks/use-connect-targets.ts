import { useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useSystemState } from '@/hooks/use-system-state'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { ensureTunReady } from '@/services/cmds'

/**
 * What the Connect button actually switches.
 *
 * System proxy and TUN are independent targets which may be driven together
 * (`verge.connect_system_proxy` / `verge.connect_tun_mode`). There is no
 * separate setting for them any more: the choice IS the pair of live switches
 * («Настройки системы» / «Быстрые действия»). Whatever the user turns on by
 * hand becomes what Connect restores next time — see `useRememberTargets`.
 * The default is the system proxy alone; when both end up off, the system
 * proxy silently steps back in — a Connect button that switches nothing is a
 * broken promise.
 *
 * `connected` reads the real state of every enabled target, never an
 * optimistic flag: a system proxy dropped from outside the app must turn the
 * button dark by itself.
 */
export const useConnectTargets = () => {
  const { t } = useTranslation()
  const { verge, patchVerge } = useVerge()
  const { isTunModeAvailable, mutateSystemState } = useSystemState()
  const { indicator: sysproxyOn, toggleSystemProxy } = useSystemProxyState()
  // clod: факт, а не желание. `enable_tun_mode` — это то, чего хочет
  // пользователь; бэкенд же может подавить TUN на сессию (ядро не смогло
  // поднять устройство, службы нет) и в конфиг при этом НЕ пишет — намеренно,
  // чтобы не терять выбор. Кнопка, читавшая конфиг, оставалась зелёной над
  // мёртвым туннелем: «Подключено», а трафик идёт мимо.
  const { tunActive, tunDesired, mutateTunState } = useTunState()

  const { targetSys, targetTun } = useMemo(() => {
    const sys = verge?.connect_system_proxy ?? true
    const tun = verge?.connect_tun_mode ?? false
    return { targetSys: sys || !tun, targetTun: tun }
  }, [verge?.connect_system_proxy, verge?.connect_tun_mode])

  const connected =
    (!targetSys || sysproxyOn) &&
    (!targetTun || tunActive) &&
    (targetSys || targetTun)

  // clod: кнопка обязана делать ровно то, что показывает. Намерение считалось
  // по ЖЕЛАНИЮ (`enable_tun_mode` в конфиге), и это ломалось на самом обычном
  // сценарии: всё выключено, пользователь включает TUN тумблером. Туннель
  // поднят, системного прокси нет — кнопка тёмная, а «что-то из целей
  // включено» превращало первое нажатие в «отключить»: TUN гасился, прокси и
  // так не было. Со стороны — нажатие не сработало, да ещё и галочка TUN
  // сбросилась сама.
  //
  // Исключение ровно одно: туннель, который бэкенд подавил (ядро не смогло
  // поднять устройство). Цели тогда не сойдутся никогда, `connected` не
  // загорится — и без оговорки кнопка умела бы только подключать, а выключить
  // поднятый системный прокси стало бы нечем. Когда же поднимать больше
  // нечего, нажатие снова означает «подключить»: повторная подача настройки
  // снимает подавление, и это единственная попытка починиться самой кнопкой.
  const tunStuck = targetTun && tunDesired && !tunActive
  const somethingUp = (targetSys && sysproxyOn) || (targetTun && tunActive)
  const willConnect = connected ? false : !(tunStuck && somethingUp)

  const toggleConnection = useCallback(async () => {
    const next = willConnect

    // clod:tun-ready — TUN нужна фоновая служба. Раньше кнопка просто ругалась
    // «установите её сами»; теперь ставим (один запрос прав) и продолжаем, а
    // ошибка остаётся только для случая, когда пользователь отказал.
    if (next && targetTun && !isTunModeAvailable && !tunActive) {
      const ready = await ensureTunReady()
      await mutateSystemState()
      if (!ready) {
        throw new Error(t('home.components.connect.errors.serviceRequired'))
      }
    }

    // Включаем — пока туннеля НЕТ (даже если в конфиге он уже «включён»:
    // повторная запись снимает сессионное подавление, переводит ядро на службу
    // и заново проверяет факт). Выключаем — пока он в конфиге есть.
    if (targetTun && (next ? !tunActive : tunDesired)) {
      await patchVerge({ enable_tun_mode: next })
      await mutateTunState()
    }
    if (targetSys && sysproxyOn !== next) {
      await toggleSystemProxy(next)
    }
  }, [
    willConnect,
    targetSys,
    targetTun,
    tunActive,
    tunDesired,
    sysproxyOn,
    isTunModeAvailable,
    mutateSystemState,
    mutateTunState,
    patchVerge,
    toggleSystemProxy,
    t,
  ])

  return { connected, willConnect, targetSys, targetTun, toggleConnection }
}

/**
 * Пользователь дёрнул живой тумблер руками — значит, именно этот режим он и
 * имеет в виду, когда жмёт Connect. Вызывать ТОЛЬКО из обработчиков самих
 * тумблеров: кнопка Connect выключает те же флаги при отключении, и если бы
 * запоминание жило внутри них, одно нажатие «отключиться» стирало бы выбор.
 */
export const useRememberTargets = () => {
  const { patchVerge } = useVerge()

  return useCallback(
    (target: 'sys' | 'tun', enabled: boolean) =>
      patchVerge(
        target === 'sys'
          ? { connect_system_proxy: enabled }
          : { connect_tun_mode: enabled },
      ).catch(() => {
        // Запоминание — вторичное действие: сам тумблер уже сработал, и
        // ронять его из-за не сохранившегося предпочтения незачем.
      }),
    [patchVerge],
  )
}
