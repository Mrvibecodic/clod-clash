import delayManager from '@/services/delay'

export interface ProxyNode {
  name: string
  type?: string
  label?: IProxyLabel
  history?: { time: string; delay: number }[]
}

export interface ProxyGroup {
  name: string
  type?: string
  now?: string
  hidden?: boolean
  all?: ProxyNode[]
}

const CORE_POLICIES = [
  'DIRECT',
  'REJECT',
  'REJECT-DROP',
  'PASS',
  'PASS-RULE',
  'COMPATIBLE',
]

/** Ядро зовёт тип встроенной политики её же именем без дефисов. */
export const NON_NODE_TYPES = new Set([
  'selector',
  'urltest',
  'fallback',
  'loadbalance',
  'smart',
  'relay',
  ...CORE_POLICIES.map((name) => name.toLowerCase().replaceAll('-', '')),
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

/** `COMPATIBLE` ядро подставляет в пустую группу само — назвать его нельзя. */
export const BUILTIN_GROUP_POLICIES = CORE_POLICIES.filter(
  (name) => name !== 'COMPATIBLE',
)

/** `PASS-RULE` в правиле верхнего уровня отбивает, а не передаёт дальше. */
export const BUILTIN_RULE_POLICIES = BUILTIN_GROUP_POLICIES.filter(
  (name) => name !== 'PASS-RULE',
)

export const isCorePolicy = (name?: string) =>
  !!name && CORE_POLICIES.includes(name)

/** `DIRECT` — осмысленный выбор человека, а не заглушка ядра. */
export const isCorePlaceholder = (name?: string) =>
  isCorePolicy(name) && name !== 'DIRECT'

export const displayLeaf = (
  records: Record<string, ProxyGroup | undefined>,
  name: string,
): string | undefined => {
  const leaf = resolveLeaf(records, name)
  if (leaf === name || isCorePlaceholder(leaf)) return undefined
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

/**
 * Пинг записи группы: её собственный замер или замер листа, куда она ведёт
 * (вложенная группа меряется по своему текущему узлу). Есть оба — берём
 * свежий: неудачная проверка группы, после которой ядро увело её на живой
 * узел и измерило его, не должна закрывать новый замер.
 */
const entryMeasurement = (
  records: Record<string, any>,
  name: string,
  group: string,
) => {
  const measure = (target: string) => {
    const record = (records[target] ?? { name: target }) as any
    return {
      target,
      delay: delayManager.getDelayFix(record, group, true),
      at: delayManager.getMeasuredAt(record, group),
    }
  }
  const own = measure(name)
  const leafName = resolveLeaf(records, name)
  if (leafName === name) return own
  const leaf = measure(leafName)
  if (own.delay < 0) return leaf
  if (leaf.delay < 0) return own
  return own.at >= leaf.at ? own : leaf
}

export const entryDelay = (
  records: Record<string, any>,
  name: string,
  group: string,
) => entryMeasurement(records, name, group).delay

export const entryPingTarget = (
  records: Record<string, any>,
  name: string,
  group: string,
) => entryMeasurement(records, name, group).target

export const entryMeasuredAt = (
  records: Record<string, any>,
  name: string,
  group: string,
) => entryMeasurement(records, name, group).at
