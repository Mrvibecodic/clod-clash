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

export const SELECTABLE_GROUP_TYPES = new Set([
  'selector',
  'urltest',
  'fallback',
])

export const AUTO_GROUP_TYPES = new Set([
  'urltest',
  'fallback',
  'loadbalance',
  'smart',
])

export const groupType = (item: { type?: string } | undefined) =>
  (item?.type ?? '').toLowerCase()

const INTERNAL_LEAF_NAMES = new Set([
  'COMPATIBLE',
  'REJECT',
  'REJECT-DROP',
  'PASS',
])

export const isCorePlaceholder = (name?: string) =>
  !!name && INTERNAL_LEAF_NAMES.has(name)

export const displayLeaf = (
  records: Record<string, ProxyGroup | undefined>,
  name: string,
): string | undefined => {
  const leaf = resolveLeaf(records, name)
  if (leaf === name || INTERNAL_LEAF_NAMES.has(leaf)) return undefined
  return leaf
}

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

export const hasRealNodes = (proxies: any): boolean => {
  const records = (proxies?.records ?? {}) as Record<string, ProxyGroup>
  return visibleGroups(proxies).some((group) =>
    (group.all ?? []).some((node) => {
      const type = groupType(records[node.name] ?? node)
      return !isCorePlaceholder(node.name) && !NON_NODE_TYPES.has(type)
    }),
  )
}

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

const DELAY_ERROR = 1e6

export const usableDelay = (delay?: number): delay is number =>
  delay !== undefined && delay > 0 && delay < DELAY_ERROR

export const failedDelay = (delay?: number): boolean =>
  delay !== undefined && (delay === 0 || delay >= DELAY_ERROR)

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
