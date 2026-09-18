import assert from 'node:assert/strict'
import { test } from 'node:test'

import { enumText } from './plugin-enum.ts'

test('enumText passes a known value through and unwraps an unknown one', () => {
  assert.equal(enumText('URLTest'), 'URLTest')
  assert.equal(enumText({ Unknown: 'NewType' }), 'NewType')
})
