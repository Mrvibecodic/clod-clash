import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { drawerRowCaption, serverRowNames } from './server-row.ts'

const words = {
  direct: 'Напрямую',
  inUse: 'Используется',
  now: (server: string) => `Сейчас: ${server}`,
}

const row = {
  selected: false,
  group: true,
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

  it('shows the node type of a server', () => {
    assert.equal(drawerRowCaption({ ...row, group: false }, words), 'VLESS')
  })
})
