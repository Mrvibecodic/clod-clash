export interface ProxyChainNode {
  id: string
  name: string
  type?: string
}

/** Цепочка принадлежит подписке: и группа, и узлы — имена из её конфига. */
export interface StoredProxyChain {
  group?: string
  exitNode?: string
  items?: ProxyChainNode[]
}

const KEY = 'proxy-chain'

const readAll = (): Record<string, StoredProxyChain> => {
  try {
    const data = JSON.parse(localStorage.getItem(KEY) ?? 'null')
    return data && typeof data === 'object' && !Array.isArray(data) ? data : {}
  } catch {
    return {}
  }
}

const writeAll = (data: Record<string, StoredProxyChain>) => {
  try {
    localStorage.setItem(KEY, JSON.stringify(data))
  } catch {}
}

const text = (value: unknown) =>
  typeof value === 'string' && value ? value : undefined

/** `id` держит и ключи списка, и перетаскивание, и удаление одного узла. */
const isNode = (value: unknown): value is ProxyChainNode =>
  !!value &&
  typeof value === 'object' &&
  typeof (value as any).id === 'string' &&
  typeof (value as any).name === 'string'

const sane = (chain: unknown): StoredProxyChain => {
  if (!chain || typeof chain !== 'object' || Array.isArray(chain)) return {}
  const { group, exitNode, items } = chain as StoredProxyChain
  return {
    group: text(group),
    exitNode: text(exitNode),
    items: Array.isArray(items) ? items.filter(isNode) : undefined,
  }
}

const isEmpty = (chain: StoredProxyChain) =>
  chain.group === undefined &&
  chain.exitNode === undefined &&
  !chain.items?.length

export const readProxyChain = (uid?: string): StoredProxyChain =>
  uid ? sane(readAll()[uid]) : {}

export const saveProxyChain = (uid: string, patch: StoredProxyChain) => {
  const all = readAll()
  const merged = sane({ ...all[uid], ...patch })
  if (isEmpty(merged)) delete all[uid]
  else all[uid] = merged
  writeAll(all)
}

export const clearProxyChain = (uid?: string) => {
  if (!uid) return
  const all = readAll()
  delete all[uid]
  writeAll(all)
}

export interface LegacyConnectedChain {
  uid: string
  group: string
  nodes: string[]
}

export const legacyConnectedChains = (): LegacyConnectedChain[] =>
  Object.entries(readAll()).flatMap(([uid, raw]) => {
    const { group, exitNode, items } = sane(raw)
    const nodes = items?.map((item) => item.name) ?? []
    return group && nodes.length >= 2 && nodes[nodes.length - 1] === exitNode
      ? [{ uid, group, nodes }]
      : []
  })

export const forgetLegacyConnections = () => {
  const all = readAll()
  const kept: Record<string, StoredProxyChain> = {}
  for (const uid of Object.keys(all)) {
    const { items } = sane(all[uid])
    if (items?.length) kept[uid] = { items }
  }
  // Зовётся на каждом чтении цепочки: без старых записей писать нечего.
  if (JSON.stringify(kept) !== JSON.stringify(all)) writeAll(kept)
}

export interface ConnectedChain {
  group: string
  nodes: string[]
  applied: boolean
}

export const chainIsConnected = (
  chain: ConnectedChain | null | undefined,
  draft: readonly string[],
  group: string | null | undefined,
) =>
  !!chain &&
  chain.applied &&
  chain.group === group &&
  draft.length >= 2 &&
  chain.nodes.length === draft.length &&
  chain.nodes.every((name, index) => name === draft[index])
