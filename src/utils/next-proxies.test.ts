import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { nextProxies } from './next-proxies.ts'

interface Snapshot {
  nodes: string[]
  labels: { stamp: string; labels?: IProxyLabels }
}

const LABELS: IProxyLabels = {
  proxies: { A: { proto: 'VLESS', text: 'VLESS' } },
  providers: {},
}
const NEWER: IProxyLabels = {
  proxies: { B: { proto: 'Trojan', text: 'Trojan' } },
  providers: {},
}

/** Разбор снимка: узлы и подписи, с которыми он собран. */
const calc = (snapshot: Snapshot, labels: IProxyLabels) => ({
  nodes: snapshot.nodes,
  builtWith: labels,
})

const shown = {
  nodes: ['A'],
  builtWith: LABELS,
  stamp: 'core-1',
  labels: { stamp: 'labels-1', labels: LABELS },
}

describe('группы и узлы после опроса ядра', () => {
  it('ядро не ответило или ничего не сменилось — остаётся показанное', () => {
    assert.equal(nextProxies(shown, null, calc), shown)
    assert.equal(nextProxies(shown, { stamp: 'core-1' }, calc), shown)
    assert.equal(nextProxies(undefined, null, calc), undefined)
  })

  it('снимок без подписей собирается с показанными подписями', () => {
    const next = nextProxies(
      shown,
      {
        stamp: 'core-2',
        snapshot: { nodes: ['A', 'B'], labels: { stamp: 'labels-1' } },
      },
      calc,
    )
    assert.deepEqual(next, {
      nodes: ['A', 'B'],
      builtWith: LABELS,
      stamp: 'core-2',
      labels: { stamp: 'labels-1', labels: LABELS },
    })
  })

  it('снимок с подписями берёт новые подписи и их отпечаток', () => {
    const next = nextProxies(
      shown,
      {
        stamp: 'core-2',
        snapshot: {
          nodes: ['B'],
          labels: { stamp: 'labels-2', labels: NEWER },
        },
      },
      calc,
    )
    assert.equal(next?.stamp, 'core-2')
    assert.deepEqual(next?.labels, { stamp: 'labels-2', labels: NEWER })
    assert.equal(next?.builtWith, NEWER)
  })

  it('первый ответ без подписей — пустые подписи, а не ошибка', () => {
    const next = nextProxies(
      undefined,
      {
        stamp: 'core-1',
        snapshot: { nodes: ['A'], labels: { stamp: 'labels-1' } },
      },
      calc,
    )
    assert.deepEqual(next?.labels.labels, { proxies: {}, providers: {} })
    assert.deepEqual(next?.builtWith, { proxies: {}, providers: {} })
  })

  it('ядро ещё не готово (разбор бросил) — остаётся показанное', () => {
    const failing = (): ReturnType<typeof calc> => {
      throw new Error('not ready')
    }
    assert.equal(
      nextProxies(
        shown,
        {
          stamp: 'core-2',
          snapshot: { nodes: [], labels: { stamp: 'labels-2' } },
        },
        failing,
      ),
      shown,
    )
  })
})
