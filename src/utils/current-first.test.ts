import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { currentFirst } from './current-first.ts'

const list = [{ name: 'a' }, { name: 'b' }, { name: 'c' }]

describe('текущий узел первым', () => {
  it('переносит текущий узел в голову, порядок остальных прежний', () => {
    assert.deepEqual(
      currentFirst(list, 'c').map((p) => p.name),
      ['c', 'a', 'b'],
    )
    assert.deepEqual(
      currentFirst(list, 'b').map((p) => p.name),
      ['b', 'a', 'c'],
    )
  })

  it('без текущего, с текущим не из списка или уже первым — список как есть', () => {
    assert.equal(currentFirst(list), list)
    assert.equal(currentFirst(list, 'x'), list)
    assert.equal(currentFirst(list, 'a'), list)
  })
})
