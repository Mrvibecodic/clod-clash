// Поля окна «Режим TUN», которые живут в `tun` файла clash.yaml. Значение поля —
// то, что закрепил человек; пустое поле не закреплено: действует подписка, а
// если она молчит — умолчание ядра.

export interface TunFields {
  device: string
  mtu: string
  routeExcludeAddress: string
  autoRoute: boolean
  autoRedirect: boolean
  autoDetectInterface: boolean
}

type TunBlock = Readonly<Record<string, unknown>> | null | undefined

export const splitRouteExcludeAddress = (value: string) =>
  value
    .split(/[,\n;\r]+/)
    .map((item) => item.trim())
    .filter(Boolean)

const text = (value: unknown) =>
  typeof value === 'string' || typeof value === 'number' ? String(value) : ''

const flag = (value: unknown) =>
  typeof value === 'boolean' ? value : undefined

/** Поля при открытии окна: закреплённое у нас, иначе пусто, а переключатели — действующее. */
export function tunFieldsFrom(
  pinned: TunBlock,
  runtimeTun: TunBlock,
  os: string,
): TunFields {
  const autoRoute = flag(pinned?.['auto-route']) ?? true
  const excluded = pinned?.['route-exclude-address']
  return {
    device: text(pinned?.device),
    mtu: text(pinned?.mtu),
    routeExcludeAddress: Array.isArray(excluded) ? excluded.join(',') : '',
    autoRoute,
    autoRedirect:
      os === 'linux' &&
      autoRoute &&
      (flag(pinned?.['auto-redirect']) ??
        flag(runtimeTun?.['auto-redirect']) ??
        false),
    autoDetectInterface: flag(pinned?.['auto-detect-interface']) ?? true,
  }
}

/** Пусто — снять закрепление; иначе целое больше нуля. */
export const mtuIsValid = (value: string) =>
  value.trim() === '' || (/^\d+$/.test(value.trim()) && Number(value) > 0)

/** Только изменённое с открытия окна; очищенное поле снимает закрепление (null). */
export function tunPatch(
  initial: TunFields,
  current: TunFields,
  os: string,
): Record<string, unknown> {
  const patch: Record<string, unknown> = {}
  if (current.device !== initial.device) {
    patch.device = current.device.trim() === '' ? null : current.device
  }
  if (current.mtu !== initial.mtu) {
    patch.mtu = current.mtu.trim() === '' ? null : Number(current.mtu)
  }
  if (current.routeExcludeAddress !== initial.routeExcludeAddress) {
    const items = splitRouteExcludeAddress(current.routeExcludeAddress)
    patch['route-exclude-address'] = items.length > 0 ? items : null
  }
  if (current.autoRoute !== initial.autoRoute) {
    patch['auto-route'] = current.autoRoute
  }
  if (os === 'linux' && current.autoRedirect !== initial.autoRedirect) {
    patch['auto-redirect'] = current.autoRedirect
  }
  if (current.autoDetectInterface !== initial.autoDetectInterface) {
    patch['auto-detect-interface'] = current.autoDetectInterface
  }
  return patch
}

/** «Сбросить» — как на свежей установке. Маршруты остаются за клиентом: без
 * auto-route туннель пуст, и трафик молча идёт напрямую. */
export const TUN_RESET: Readonly<Record<string, unknown>> = {
  device: null,
  mtu: null,
  'route-exclude-address': null,
  'auto-redirect': null,
  'auto-route': true,
  'auto-detect-interface': true,
}
