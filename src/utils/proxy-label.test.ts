import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import {
  ambiguousNames,
  featureChips,
  hidesType,
  labelFor,
  own,
  typeChips,
  typeText,
} from './proxy-label.ts'

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

describe('clod-hide-badges', () => {
  const label = {
    proto: 'VLESS',
    transport: 'gRPC',
    security: 'TLS',
    text: 'VLESS gRPC · TLS',
  }

  it('hides every badge when the provider asks', () => {
    assert.deepEqual(typeChips({ type: 'Vless', label }, true), [])
    assert.deepEqual(typeChips({ type: 'Vless' }, true), [])
    assert.equal(typeText({ type: 'Vless', label }, true), '')
    assert.equal(typeText({ type: 'Vless' }, true), '')
  })

  it('keeps the type of a group and hides that of DIRECT and REJECT', () => {
    const group = { type: 'Selector', all: ['A', 'B'] }
    assert.deepEqual(typeChips(group, true), ['Selector'])
    assert.equal(typeText({ type: 'URLTest', all: ['A'] }, true), 'URLTest')
    assert.deepEqual(typeChips({ type: 'Direct' }, true), [])
    assert.equal(typeText({ type: 'Reject' }, true), '')
    assert.equal(hidesType(undefined, true), true)
    assert.equal(hidesType(group, false), false)
  })

  it('hides TFO, MPTCP and SMUX with them', () => {
    const all = { tfo: true, mptcp: true, smux: true }
    assert.deepEqual(featureChips(all), ['TFO', 'MPTCP', 'SMUX'])
    assert.deepEqual(featureChips({ tfo: false, mptcp: false, smux: true }), [
      'SMUX',
    ])
    assert.deepEqual(featureChips(all, true), [])
  })

  it('shows them as before otherwise', () => {
    assert.deepEqual(typeChips({ type: 'Vless', label }, false), [
      'VLESS',
      'gRPC',
      'TLS',
    ])
    assert.equal(typeText({ type: 'Vless', label }), 'VLESS gRPC · TLS')
    assert.equal(typeText({ type: 'Vless' }), 'Vless')
    assert.equal(typeText({}), '')
  })

  it('every server row draws its badges through the flag', () => {
    const read = (path: string) =>
      readFileSync(new URL(path, import.meta.url), 'utf8')
    for (const file of [
      '../components/proxy/proxy-item.tsx',
      '../components/proxy/proxy-item-mini.tsx',
    ]) {
      const source = read(file)
      assert.match(source, /typeChips\(proxy, hideBadges\)/, file)
      assert.doesNotMatch(source, /typeChips\(proxy\)/, file)
      assert.match(source, /featureChips\(proxy, hideBadges\)/, file)
      assert.doesNotMatch(source, /proxy\.(tfo|mptcp|smux)/, file)
    }
    const render = read('../components/proxy/proxy-render.tsx')
    assert.equal(
      render.match(/hideBadges=\{hideBadges\}/g)?.length,
      2,
      'обе строки списка получают флаг',
    )
    for (const file of [
      '../components/proxy/proxy-groups.tsx',
      '../components/proxy/proxy-groups-chain.tsx',
    ]) {
      const source = read(file)
      assert.match(source, /hide_badges/, file)
    }
    for (const file of [
      '../components/proxy/proxy-chain.tsx',
      '../components/home/server-select.tsx',
    ]) {
      const source = read(file)
      assert.match(source, /current\?\.hide_badges/, file)
      assert.doesNotMatch(source, /label\?\.text \?\? node\.type/, file)
    }
    // Тип группы остаётся везде: на Главной — по составу из ответа ядра, в
    // цепочке — состав передаётся в подпись.
    assert.match(
      read('../components/home/server-select.tsx'),
      /isGroup && !hidesType\(record, current\?\.hide_badges\)/,
    )
    assert.match(
      read('../components/proxy/proxy-chain.tsx'),
      /all: proxies\?\.records\?\.\[proxy\.name\]\?\.all/,
    )
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
