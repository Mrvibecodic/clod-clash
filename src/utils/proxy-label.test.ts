import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { ambiguousNames, labelFor, own, typeChips } from './proxy-label.ts'

describe('typeChips', () => {
  it('shows protocol, transport and security in that order', () => {
    const label = {
      proto: 'VLESS',
      transport: 'RAW (TCP)',
      security: 'Reality',
      text: 'VLESS RAW (TCP) · Reality',
    }
    assert.deepEqual(typeChips({ type: 'Vless', label }), [
      'VLESS',
      'RAW (TCP)',
      'Reality',
    ])
  })

  it('leaves out what the server does not have', () => {
    const label = { proto: 'Shadowsocks', text: 'Shadowsocks' }
    assert.deepEqual(typeChips({ type: 'Shadowsocks', label }), ['Shadowsocks'])
  })

  it('keeps the core type when the profile says nothing', () => {
    assert.deepEqual(typeChips({ type: 'Vless' }), ['Vless'])
    assert.deepEqual(typeChips({ type: 'Direct' }), ['Direct'])
  })
})

describe('labelFor', () => {
  const vless = { proto: 'VLESS', transport: 'gRPC', text: 'VLESS gRPC' }

  it('keeps a label for the protocol the core runs', () => {
    assert.equal(labelFor('Vless', vless), vless)
    const socks = { proto: 'SOCKS', text: 'SOCKS' }
    assert.equal(labelFor('Socks5', socks), socks)
  })

  it('drops a label about another protocol', () => {
    assert.equal(labelFor('Shadowsocks', vless), undefined)
    assert.equal(labelFor('Vless', undefined), undefined)
  })
})

describe('own', () => {
  it('ignores what the prototype has', () => {
    const map: Record<string, number> = JSON.parse('{"__proto__": 1, "a": 2}')
    assert.equal(own(map, '__proto__'), 1)
    assert.equal(own(map, 'a'), 2)
    for (const name of ['constructor', 'toString', 'hasOwnProperty']) {
      assert.equal(own(map, name), undefined)
    }
    assert.equal(own(undefined, 'a'), undefined)
  })

  it('keeps labelFor safe from such names', () => {
    const labels: Record<string, IProxyLabel> = {}
    assert.equal(labelFor('Vless', own(labels, 'constructor')), undefined)
    const odd = { toString: 1 } as unknown as IProxyLabel
    assert.equal(labelFor('Vless', odd), undefined)
  })
})

describe('ambiguousNames', () => {
  const a = { proto: 'VLESS', transport: 'gRPC', text: 'VLESS gRPC' }
  const b = { proto: 'VLESS', transport: 'XHTTP', text: 'VLESS XHTTP' }

  it('marks names whose sources disagree', () => {
    const out = ambiguousNames([
      ['same', a],
      ['same', a],
      ['differs', a],
      ['differs', b],
      ['half', a],
      ['half', undefined],
      ['alone', b],
      ['constructor', a],
    ])
    assert.deepEqual([...out].sort(), ['differs', 'half'])
  })
})

describe('ambiguousNames with inline providers', () => {
  it('drops the label when an inline provider node shares the name', () => {
    const grpc = { proto: 'VLESS', transport: 'gRPC', text: 'VLESS gRPC' }
    const ws = {
      proto: 'VLESS',
      transport: 'WebSocket',
      text: 'VLESS WebSocket',
    }
    // Узел подписки X и узел встроенного провайдера X — группа может брать любой.
    const fromProfile = [['X', grpc]] as const
    const fromInline = [['X', ws]] as const
    assert.deepEqual(
      [...ambiguousNames([...fromProfile, ...fromInline])],
      ['X'],
    )
    // Встроенный провайдер без подписи — тоже расхождение.
    assert.deepEqual(
      [...ambiguousNames([...fromProfile, ['X', undefined]])],
      ['X'],
    )
  })
})
