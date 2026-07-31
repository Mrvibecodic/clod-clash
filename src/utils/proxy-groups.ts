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
 * Follow a chain of groups down to the node that actually carries traffic:
 * a selector may point at a balancer, which points at a server. Cycles and
 * dead ends fall back to the last resolvable name.
 */
export const resolveLeaf = (
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
