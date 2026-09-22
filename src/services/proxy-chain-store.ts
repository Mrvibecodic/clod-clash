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

/**
 * Прежняя цепочка лежала тремя общими ключами. Она была собрана под ту
 * подписку, что выбрана сейчас, — ей и достаётся, остальные начнут со своей.
 */
const takeTheSharedChain = (uid: string): StoredProxyChain | undefined => {
  try {
    const [group, exitNode, items] = SHARED_KEYS.map((key) =>
      localStorage.getItem(key),
    )
    for (const key of SHARED_KEYS) localStorage.removeItem(key)
    if (!group && !exitNode && !items) return undefined
    const parsed = items ? JSON.parse(items) : null
    const chain: StoredProxyChain = {
      group: group ?? undefined,
      exitNode: exitNode ?? undefined,
      items: Array.isArray(parsed) ? parsed : undefined,
    }
    saveProxyChain(uid, chain)
    return chain
  } catch {
    return undefined
  }
}

export const readProxyChain = (uid?: string): StoredProxyChain => {
  if (!uid) return {}
  const stored = readAll()[uid]
  const chain = stored ?? takeTheSharedChain(uid) ?? {}
  return Array.isArray(chain.items) ? chain : { ...chain, items: undefined }
}

export const saveProxyChain = (uid: string, patch: StoredProxyChain) => {
  const all = readAll()
  all[uid] = { ...all[uid], ...patch }
  writeAll(all)
}

export const clearProxyChain = (uid?: string) => {
  if (!uid) return
  const all = readAll()
  delete all[uid]
  writeAll(all)
}
