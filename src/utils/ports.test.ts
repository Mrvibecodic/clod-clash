import assert from 'node:assert/strict'
import { test } from 'node:test'

import { findDuplicatePort } from './ports.ts'

test('находит первый повтор и называет именно его', () => {
  assert.equal(findDuplicatePort([7897, 7898, 7899, 7898]), 7898)
  assert.equal(findDuplicatePort([7897, 7897]), 7897)
})

test('без повторов ничего не находит', () => {
  assert.equal(findDuplicatePort([]), undefined)
  assert.equal(findDuplicatePort([7897]), undefined)
  assert.equal(findDuplicatePort([7897, 7898, 7899]), undefined)
})
