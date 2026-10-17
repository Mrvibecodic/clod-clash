import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { selectionEnd, serverBehind } from './server-behind.ts'

const records = {
  Proxy: { now: 'Auto', all: [] },
  Auto: { now: 'Hidden', all: [] },
  Hidden: { now: '🇳🇱 Netherlands', all: [] },
  '🇳🇱 Netherlands': {},
  Direct: { now: 'DIRECT', all: [] },
  DIRECT: {},
  Empty: { now: 'COMPATIBLE', all: [] },
  COMPATIBLE: {},
  Balance: { all: [] },
  ToBalance: { now: 'Balance', all: [] },
  A: { now: 'B', all: [] },
  B: { now: 'A', all: [] },
}
const placeholder = (name: string) => name === 'COMPATIBLE'

describe('serverBehind', () => {
  it('names the server and the group it is selected in, past hidden groups', () => {
    assert.deepEqual(serverBehind(records, 'Proxy', placeholder), {
      server: '🇳🇱 Netherlands',
      via: 'Hidden',
    })
    assert.deepEqual(serverBehind(records, 'Hidden', placeholder), {
      server: '🇳🇱 Netherlands',
      via: 'Hidden',
    })
  })

  it('treats DIRECT as where the group stands', () => {
    assert.deepEqual(serverBehind(records, 'Direct', placeholder), {
      server: 'DIRECT',
      via: 'Direct',
    })
  })

  it('has no server for a server, a core placeholder or a path ending in a group', () => {
    assert.equal(
      serverBehind(records, '🇳🇱 Netherlands', placeholder),
      undefined,
    )
    assert.equal(serverBehind(records, 'Empty', placeholder), undefined)
    assert.equal(serverBehind(records, 'Balance', placeholder), undefined)
    assert.equal(serverBehind(records, 'ToBalance', placeholder), undefined)
    assert.equal(serverBehind(records, 'A', placeholder), undefined)
  })
})

describe('selectionEnd', () => {
  it('stops at a group that selects nothing and on a loop', () => {
    assert.deepEqual(selectionEnd(records, 'Balance'), {
      leaf: 'Balance',
      via: undefined,
    })
    assert.equal(selectionEnd(records, 'A').leaf, 'A')
    assert.equal(selectionEnd(records, 'Unknown').leaf, 'Unknown')
  })
})
