export interface ProxyChainNode {
  id: string
  name: string
  type?: string
  delay?: number
}

/** Цепочка принадлежит подписке: и группа, и узлы — имена из её конфига. */
export interface StoredProxyChain {
  group?: string
  exitNode?: string
  items?: ProxyChainNode[]
}

const KEY = 'proxy-chain'
const SHARED_KEYS = [
  'proxy-chain-group',
  'proxy-chain-exit-node',
  'proxy-chain-items',
] as const

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

/**
 * Прежняя цепочка лежала тремя общими ключами. Забираем их при первом же
 * обращении, чтобы они не достались позже ЧУЖОЙ подписке; отдаём только той,
 * у которой своей записи ещё нет.
 */
const takeTheSharedChain = (): StoredProxyChain | undefined => {
  const [group, exitNode, items] = SHARED_KEYS.map((key) => {
    try {
      return localStorage.getItem(key)
    } catch {
      return null
    }
  })
  if (group === null && exitNode === null && items === null) return undefined

  let parsed: unknown
  try {
    parsed = items ? JSON.parse(items) : undefined
  } catch {
    parsed = undefined
  }
  for (const key of SHARED_KEYS) {
    try {
      localStorage.removeItem(key)
    } catch {}
  }
  return sane({ group, exitNode, items: parsed } as StoredProxyChain)
}

export const readProxyChain = (uid?: string): StoredProxyChain => {
  if (!uid) return {}
  const all = readAll()
  const shared = takeTheSharedChain()
  if (!all[uid] && shared && !isEmpty(shared)) {
    all[uid] = shared
    writeAll(all)
    return shared
  }
  return sane(all[uid])
}

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
