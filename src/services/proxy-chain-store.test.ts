import assert from 'node:assert/strict'
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

const { clearProxyChain, readProxyChain, saveProxyChain } = await import(
  './proxy-chain-store.ts'
)

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

  it('has nothing to say without a subscription', () => {
    saveProxyChain('A', { items: [node('a1')] })
    assert.equal(readProxyChain(undefined).items, undefined)
    clearProxyChain(undefined)
    assert.deepEqual(readProxyChain('A').items, [node('a1')])
  })
})
