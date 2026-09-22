interface ProxyChainNode {
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

const readAll = (): Record<string, StoredProxyChain> => {
  try {
    const raw = localStorage.getItem(KEY)
    const data = raw ? JSON.parse(raw) : null
    return data && typeof data === 'object' ? data : {}
  } catch {
    return {}
  }
}

const writeAll = (data: Record<string, StoredProxyChain>) => {
  try {
    localStorage.setItem(KEY, JSON.stringify(data))
  } catch {}
}

export const readProxyChain = (uid?: string): StoredProxyChain =>
  uid ? (readAll()[uid] ?? {}) : {}

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
