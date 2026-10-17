import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { resolveSimpleMode } from './simple-mode.ts'

describe('режим интерфейса', () => {
  it('выбор человека, затем провайдера, иначе простой', () => {
    const table: [boolean | undefined, boolean | undefined, boolean][] = [
      [undefined, undefined, true],
      [undefined, false, false],
      [undefined, true, true],
      [true, false, true],
      [false, true, false],
    ]
    for (const [user, provider, simple] of table) {
      assert.equal(
        resolveSimpleMode(user, provider),
        simple,
        `${user}/${provider}`,
      )
    }
  })
})
