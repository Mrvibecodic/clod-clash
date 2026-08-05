import delayManager from '@/services/delay'

export interface ProxyNode {
  name: string
  type?: string
  history?: { time: string; delay: number }[]
}

export interface ProxyGroup {
  name: string
  type?: string
  now?: string
  hidden?: boolean
  all?: ProxyNode[]
}

/** Entry types that are groups or built-ins, not actual servers. */
export const NON_NODE_TYPES = new Set([
  'selector',
  'urltest',
  'fallback',
  'loadbalance',
  'smart',
  'relay',
  'direct',
  'reject',
  'rejectdrop',
  'pass',
  'compatible',
])

/** Group types where the core accepts a manual selection. */
export const SELECTABLE_GROUP_TYPES = new Set([
  'selector',
  'urltest',
  'fallback',
])

/** Balancer-style groups: the core picks the node, the name is a strategy. */
export const AUTO_GROUP_TYPES = new Set([
  'urltest',
  'fallback',
  'loadbalance',
  'smart',
])

export const groupType = (item: { type?: string } | undefined) =>
  (item?.type ?? '').toLowerCase()

/**
 * Internal placeholder names the core substitutes when a group has nothing
 * real to point at (e.g. an untested urltest resolves to `COMPATIBLE`).
 * They mean nothing to the user, so captions and flags must not show them.
 */
const INTERNAL_LEAF_NAMES = new Set([
  'COMPATIBLE',
  'REJECT',
  'REJECT-DROP',
  'PASS',
])

/**
 * clod: `REJECT` and friends can end up as a group's only member — that is
 * what the sentinel filter leaves behind when the panel sent nothing but
 * placeholder nodes (expired subscription). They are not servers and must not
 * be offered as a choice; an empty list reads far better than a "REJECT" row.
 *
 * `DIRECT` is deliberately not here: templates ship it as a real, pickable
 * option ("no VPN" groups).
 */
export const isCorePlaceholder = (name?: string) =>
  !!name && INTERNAL_LEAF_NAMES.has(name)

/**
 * The leaf worth showing next to a group: the resolved node, or `undefined`
 * when the chain ends where it started or lands on a core placeholder.
 */
export const displayLeaf = (
  records: Record<string, ProxyGroup | undefined>,
  name: string,
): string | undefined => {
  const leaf = resolveLeaf(records, name)
  if (leaf === name || INTERNAL_LEAF_NAMES.has(leaf)) return undefined
  return leaf
}

/**
 * Groups worth showing to the user: everything the template author did not
 * hide — selectors and balancers alike, in the template's own order.
 *
 * Panels often strip the template down to a flat `proxies:` list with no
 * custom groups at all (mode: global does the routing). The built-in GLOBAL
 * selector is the only way to pick a server there, so it serves as the
 * fallback — with the group entries filtered out, leaving actual servers.
 */
export const visibleGroups = (proxies: any): ProxyGroup[] => {
  const groups = ((proxies?.groups ?? []) as ProxyGroup[]).filter(
    (group) => !group.hidden && group.name !== 'GLOBAL',
  )
  if (groups.length > 0) return groups

  const global = proxies?.global as ProxyGroup | undefined
  const records = proxies?.records ?? {}
  const nodes = (global?.all ?? []).filter((node) => {
    const type = (records[node.name]?.type ?? node.type ?? '').toLowerCase()
    return !NON_NODE_TYPES.has(type)
  })
  return nodes.length > 0 ? [{ ...global, name: 'GLOBAL', all: nodes }] : []
}

/**
 * clod: есть ли в конфиге хоть один настоящий сервер.
 *
 * Считаем по **всем** видимым группам, а не по выбранной: у шаблонов бывает
 * группа-маршрутизатор из одних подгрупп и группа «без VPN» из одного `DIRECT`,
 * и по любой из них поодиночке вышло бы «серверов нет», пока в соседней лежит
 * рабочий список. Один ответ на двоих: строка на главной и шторка не должны
 * расходиться в том, что видит пользователь.
 */
export const hasRealNodes = (proxies: any): boolean => {
  const records = (proxies?.records ?? {}) as Record<string, ProxyGroup>
  return visibleGroups(proxies).some((group) =>
    (group.all ?? []).some((node) => {
      const type = groupType(records[node.name] ?? node)
      return !isCorePlaceholder(node.name) && !NON_NODE_TYPES.has(type)
    }),
  )
}

/**
 * Follow a chain of groups down to the node that actually carries traffic:
 * a selector may point at a balancer, which points at a server. Cycles and
 * dead ends fall back to the last resolvable name.
 */
const resolveLeaf = (
  records: Record<string, ProxyGroup | undefined>,
  name: string,
): string => {
  let current = name
  for (let hop = 0; hop < 6; hop += 1) {
    const record = records[current]
    if (!record?.now || record.now === current) break
    current = record.now
  }
  return current
}

/**
 * Delay history stores `0` for a failed test; `getDelayFix` maps it to this
 * marker so it sorts last. It is an error state, not a number to display.
 */
const DELAY_ERROR = 1e6

/** A delay worth showing as a number: positive and not the failure marker. */
export const usableDelay = (delay?: number): delay is number =>
  delay !== undefined && delay > 0 && delay < DELAY_ERROR

/** Delay of an entry, resolving groups down to their active node. */
export const entryDelay = (
  records: Record<string, any>,
  name: string,
  group: string,
) => {
  const record = records[name]
  const direct = delayManager.getDelayFix((record ?? { name }) as any, group)
  if (direct > 0) return direct
  const leaf = resolveLeaf(records, name)
  if (leaf === name) return direct
  return delayManager.getDelayFix(
    (records[leaf] ?? { name: leaf }) as any,
    group,
  )
}

/**
 * Чья задержка показана в строке: сама запись или лист, до которого дошла
 * цепочка. Ровно то же правило, что и в `entryDelay`, — иначе перемерить
 * можно не тот узел, чья цифра висит на экране.
 */
export const entryPingTarget = (
  records: Record<string, any>,
  name: string,
  group: string,
) => {
  const direct = delayManager.getDelayFix(
    (records[name] ?? { name }) as any,
    group,
  )
  if (direct > 0) return name
  return resolveLeaf(records, name)
}

/** Когда сняли показанный пинг записи: мс epoch, 0 — не знаем. */
export const entryMeasuredAt = (
  records: Record<string, any>,
  name: string,
  group: string,
) => {
  const target = entryPingTarget(records, name, group)
  return delayManager.getMeasuredAt(
    (records[target] ?? { name: target }) as any,
    group,
  )
}
