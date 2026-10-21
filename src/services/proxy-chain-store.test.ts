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

  it('hands the old shared chain to the subscription that asks first', () => {
    store.set('proxy-chain-group', 'GLOBAL')
    store.set('proxy-chain-exit-node', 'exit')
    store.set('proxy-chain-items', JSON.stringify([node('n1'), node('n2')]))

    const mine = readProxyChain('A')

    assert.equal(mine.group, 'GLOBAL')
    assert.equal(mine.exitNode, 'exit')
    assert.deepEqual(mine.items, [node('n1'), node('n2')])
    assert.equal(
      store.get('proxy-chain-items'),
      undefined,
      'общие ключи уходят',
    )
    assert.equal(
      readProxyChain('B').items,
      undefined,
      'второй подписке прежняя цепочка уже не достаётся',
    )
    assert.deepEqual(readProxyChain('A').items, [node('n1'), node('n2')])
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

  it('never lets the old shared chain reach a later subscription', () => {
    saveProxyChain('A', { items: [node('a1')] })
    store.set('proxy-chain-group', 'GLOBAL')
    store.set('proxy-chain-exit-node', 'exit')

    readProxyChain('A')

    assert.equal(
      store.get('proxy-chain-group'),
      undefined,
      'общие ключи забраны сразу',
    )
    assert.equal(readProxyChain('B').exitNode, undefined)
  })

  it('does not lose the group when the old node list is broken', () => {
    store.set('proxy-chain-group', 'GLOBAL')
    store.set('proxy-chain-exit-node', 'N3')
    store.set('proxy-chain-items', '[{сломано')

    const mine = readProxyChain('A')

    assert.equal(mine.group, 'GLOBAL')
    assert.equal(mine.exitNode, 'N3')
    assert.equal(mine.items, undefined)
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

  it('treats an empty string in the old keys as nothing', () => {
    store.set('proxy-chain-group', '')
    store.set('proxy-chain-exit-node', '')

    readProxyChain('A')

    assert.equal(
      store.get('proxy-chain'),
      undefined,
      'записи-призрака быть не должно',
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
