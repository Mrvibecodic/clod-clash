import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { beforeEach, describe, it } from 'node:test'

const store = new Map<string, string>()
globalThis.localStorage = {
  getItem: (key: string) => store.get(key) ?? null,
  setItem: (key: string, value: string) => void store.set(key, value),
  removeItem: (key: string) => void store.delete(key),
  clear: () => store.clear(),
  key: (index: number) => [...store.keys()][index] ?? null,
  get length() {
    return store.size
  },
} as Storage

const {
  chainIsConnected,
  clearProxyChain,
  forgetLegacyConnections,
  legacyConnectedChains,
  readProxyChain,
  saveProxyChain,
} = await import('./proxy-chain-store.ts')

const node = (name: string) => ({ id: name, name })

describe('proxy chain store', () => {
  beforeEach(() => store.clear())

  it('keeps every subscription chain apart', () => {
    saveProxyChain('A', { group: 'GA', items: [node('a1')] })
    saveProxyChain('B', { group: 'GB', items: [node('b1')] })

    assert.deepEqual(readProxyChain('A').items, [node('a1')])
    assert.equal(readProxyChain('B').group, 'GB')
    clearProxyChain('A')
    assert.equal(readProxyChain('A').items, undefined)
    assert.equal(readProxyChain('A').group, undefined)
    assert.equal(readProxyChain('B').group, 'GB')
  })

  it('survives a mangled record', () => {
    store.set('proxy-chain', '{"A": {"items": "не список"}}')
    assert.equal(readProxyChain('A').items, undefined)

    store.set('proxy-chain', 'не json')
    assert.equal(readProxyChain('A').items, undefined)
  })

  it('keeps the group and the exit node when the node list empties', () => {
    saveProxyChain('A', {
      group: 'GLOBAL',
      exitNode: 'N3',
      items: [node('n1'), node('n2')],
    })

    saveProxyChain('A', { items: undefined })

    const mine = readProxyChain('A')
    assert.equal(
      mine.exitNode,
      'N3',
      'иначе поднятую цепочку не видно и нечем разорвать',
    )
    assert.equal(mine.group, 'GLOBAL')
    assert.equal(mine.items, undefined)
  })

  it('leaves no record behind when there is nothing left to remember', () => {
    saveProxyChain('A', { items: [node('n1')] })
    saveProxyChain('A', { items: undefined })

    assert.equal(store.get('proxy-chain'), '{}')
  })

  it('drops entries that are not nodes', () => {
    store.set(
      'proxy-chain',
      '{"A":{"items":[{"id":"1","name":"n1"},null,7,{"name":"без id"}]}}',
    )

    assert.deepEqual(
      readProxyChain('A').items,
      [{ id: '1', name: 'n1' }],
      'на id держатся и ключи списка, и перетаскивание, и удаление одного узла',
    )
  })

  it('has nothing to say without a subscription', () => {
    saveProxyChain('A', { items: [node('a1')] })
    assert.equal(readProxyChain(undefined).items, undefined)
    clearProxyChain(undefined)
    assert.deepEqual(readProxyChain('A').items, [node('a1')])
  })
})

describe('chains connected by earlier versions', () => {
  beforeEach(() => store.clear())

  it('are handed over once with their group and nodes, the draft stays', () => {
    saveProxyChain('A', {
      group: 'Proxy',
      exitNode: 'b',
      items: [node('a'), node('b')],
    })
    saveProxyChain('B', { items: [node('x'), node('y')] })
    saveProxyChain('C', {
      group: 'Proxy',
      exitNode: 'gone',
      items: [node('a')],
    })

    assert.deepEqual(legacyConnectedChains(), [
      { uid: 'A', group: 'Proxy', nodes: ['a', 'b'] },
    ])
    forgetLegacyConnections()
    assert.deepEqual(legacyConnectedChains(), [])
    assert.deepEqual(readProxyChain('A'), {
      group: undefined,
      exitNode: undefined,
      items: [node('a'), node('b')],
    })
    assert.deepEqual(readProxyChain('B').items, [node('x'), node('y')])
    assert.deepEqual(readProxyChain('C').items, [node('a')])
  })

  it('are not rewritten on every read when there is nothing to forget', () => {
    saveProxyChain('A', { items: [node('a'), node('b')] })
    const before = store.get('proxy-chain')
    let writes = 0
    const setItem = localStorage.setItem
    localStorage.setItem = (key: string, value: string) => {
      writes += 1
      setItem(key, value)
    }
    try {
      forgetLegacyConnections()
    } finally {
      localStorage.setItem = setItem
    }
    assert.equal(writes, 0)
    assert.equal(store.get('proxy-chain'), before)
  })
})

describe('chainIsConnected', () => {
  const chain = { group: 'Proxy', nodes: ['a', 'b'], applied: true }

  it('is connected exactly when the accepted config carries the chain', () => {
    assert.equal(chainIsConnected(chain, ['a', 'b'], 'Proxy'), true)
    assert.equal(
      chainIsConnected({ ...chain, applied: false }, ['a', 'b'], 'Proxy'),
      false,
    )
    assert.equal(chainIsConnected(null, ['a', 'b'], 'Proxy'), false)
  })

  it('is not connected for another group or another list', () => {
    assert.equal(chainIsConnected(chain, ['a', 'b'], 'Auto'), false)
    assert.equal(chainIsConnected(chain, ['a', 'c'], 'Proxy'), false)
    assert.equal(chainIsConnected(chain, ['a', 'b', 'c'], 'Proxy'), false)
  })
})

describe('leaving the chain mode', () => {
  it('keeps the mode on and tells why when the chain did not come off', () => {
    const page = readFileSync(
      new URL('../pages/proxies.tsx', import.meta.url),
      'utf8',
    )
    const off = page.indexOf('await disconnectProxyChain()')
    const failed = page.indexOf('showNotice.error(error)', off)
    const left = page.indexOf('setIsChainMode(newChainMode)')
    assert.ok(off > 0 && failed > off && left > failed)
  })
})

describe('a refused connection', () => {
  it('tells the reason the backend gave', () => {
    const chain = readFileSync(
      new URL('../components/proxy/proxy-chain.tsx', import.meta.url),
      'utf8',
    )
    assert.match(
      chain,
      /showNotice\.error\('proxies\.page\.chain\.connectFailed', \{\s*reason: errorReason\(error, t\),?\s*\}\)/,
    )
  })
})
