import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  clashMode,
  drawerRowCaption,
  serverRowNames,
  trafficGroups,
} from './server-row.ts'

const words = {
  direct: 'Напрямую',
  inUse: 'Используется',
  now: (server: string) => `Сейчас: ${server}`,
}

const row = {
  selected: false,
  group: true,
  builtin: false,
  typeShown: true,
  typeLabel: 'группа',
  nodeType: 'VLESS',
}

describe('serverRowNames', () => {
  it('names the direct connection with the word of the window', () => {
    assert.equal(
      serverRowNames('DIRECT', undefined, undefined, 'Напрямую').name,
      'Напрямую',
    )
    const viaGroup = serverRowNames(
      'Auto',
      { server: 'DIRECT', via: 'Auto' },
      'DIRECT',
      'Напрямую',
    )
    assert.equal(viaGroup.name, 'Напрямую')
    assert.equal(viaGroup.leaf, 'Напрямую')
    assert.equal(viaGroup.via, 'Auto')
  })

  it('names a server and its group without flag emoji', () => {
    const names = serverRowNames(
      '🇳🇱 Proxy',
      { server: '🇩🇪 Berlin', via: '🇳🇱 Proxy' },
      '🇩🇪 Berlin',
      'Напрямую',
    )
    assert.deepEqual(names, { name: 'Berlin', via: 'Proxy', leaf: 'Berlin' })
  })
})

describe('drawerRowCaption', () => {
  it('names the direct connection behind a group with the word of the window', () => {
    assert.equal(
      drawerRowCaption(
        { ...row, behind: { server: 'DIRECT', via: 'Auto' } },
        words,
      ),
      'Сейчас: Напрямую',
    )
    assert.equal(
      drawerRowCaption({ ...row, selected: true, leaf: 'DIRECT' }, words),
      'Используется · Напрямую',
    )
    assert.equal(
      drawerRowCaption({ ...row, leaf: 'DIRECT' }, words),
      'группа · Напрямую',
    )
  })

  it('gives the built-in direct connection no type of the core', () => {
    const direct = { ...row, builtin: true, typeLabel: 'direct', nodeType: '' }
    assert.equal(drawerRowCaption(direct, words), '')
    assert.equal(
      drawerRowCaption({ ...direct, selected: true }, words),
      'Используется',
    )
  })

  it('shows the node type of a server', () => {
    assert.equal(drawerRowCaption({ ...row, group: false }, words), 'VLESS')
  })
})

describe('trafficGroups', () => {
  const visible = ['Proxy', 'Auto']

  it('in the rule mode the groups of the subscription decide, the first one first', () => {
    assert.deepEqual(trafficGroups('rule', visible, 'GLOBAL'), visible)
  })

  it('in the global mode GLOBAL decides', () => {
    assert.deepEqual(trafficGroups('global', visible, 'GLOBAL'), ['GLOBAL'])
  })

  it('in the direct mode no group decides', () => {
    assert.deepEqual(trafficGroups('direct', visible, 'GLOBAL'), [])
  })

  it('reads the mode of the core in any case and falls back to rules', () => {
    assert.equal(clashMode('Global'), 'global')
    assert.equal(clashMode('DIRECT'), 'direct')
    assert.equal(clashMode('rule'), 'rule')
    assert.equal(clashMode(undefined), 'rule')
    assert.equal(clashMode('script'), 'rule')
  })
})
