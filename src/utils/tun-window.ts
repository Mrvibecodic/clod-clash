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

/** Перехват DNS без единой записи — «как в подписке», а не «не перехватывать». */
export const dnsHijackSetting = (value = 'auto') =>
  value.split(',').some((entry) => entry.trim() !== '') ? value : 'auto'

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
    patch.mtu =
      current.mtu.trim() === ''
        ? null
        : mtuIsValid(current.mtu)
          ? Number(current.mtu)
          : current.mtu
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

export function tunSavePatch(
  initial: TunFields,
  current: TunFields,
  reset: TunFields | null,
  os: string,
): Record<string, unknown> {
  return reset
    ? { ...TUN_RESET, ...tunPatch(reset, current, os) }
    : tunPatch(initial, current, os)
}

export interface TunOverrideFields {
  stack: string
  strictRoute: string
  dnsHijack: string
}

export const tunOverrides = (fields: TunOverrideFields) => ({
  tun_stack: fields.stack,
  tun_strict_route: fields.strictRoute,
  tun_dns_hijack: dnsHijackSetting(fields.dnsHijack),
})

const sameValue = (left: unknown, right: unknown) =>
  JSON.stringify(left ?? null) === JSON.stringify(right ?? null)

/** Сколько записей поменяет «Сохранить» — счёт от сохранённого на диске, а не
 * от формы: форма не отличает снятое закрепление от закреплённого умолчания. */
export function tunUnsavedCount(
  pinned: TunBlock,
  patch: Record<string, unknown>,
  savedOverrides: Record<string, unknown>,
  overrides: Record<string, unknown>,
): number {
  const differs = (
    saved: Readonly<Record<string, unknown>> | null | undefined,
    next: Record<string, unknown>,
  ) =>
    Object.entries(next).filter(
      ([key, value]) => !sameValue(saved?.[key], value),
    ).length
  return differs(pinned, patch) + differs(savedOverrides, overrides)
}

export const STACKS = [
  { value: 'system', label: 'System' },
  { value: 'gvisor', label: 'gVisor' },
  { value: 'mixed', label: 'Mixed' },
  { value: 'mips', label: 'MIPS' },
]

export const stackLabel = (value: string) =>
  STACKS.find((stack) => stack.value === value.toLowerCase())?.label ?? value
