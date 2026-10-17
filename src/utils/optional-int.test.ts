import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { optionalInt } from './optional-int.ts'

describe('числовое поле формы', () => {
  it('пусто и не число — не задано, иначе целое', () => {
    assert.equal(optionalInt(''), undefined)
    assert.equal(optionalInt('  '), undefined)
    assert.equal(optionalInt('abc'), undefined)
    assert.equal(optionalInt('5000'), 5000)
    assert.equal(optionalInt(' 42 '), 42)
  })
})
