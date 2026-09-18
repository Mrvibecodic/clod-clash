import assert from 'node:assert/strict'
import { test } from 'node:test'

import { pickKnownKeys, pickStringMap } from './helpers.ts'

test('pickKnownKeys keeps only the listed keys', () => {
  assert.deepEqual(
    pickKnownKeys({ mode: 'websocket', evil: 1, host: 'a' }, ['mode', 'host']),
    { mode: 'websocket', host: 'a' },
  )
})

test('pickKnownKeys returns an empty object for anything that is not an object', () => {
  assert.deepEqual(pickKnownKeys('text', ['mode']), {})
  assert.deepEqual(pickKnownKeys(['mode'], ['mode']), {})
  assert.deepEqual(pickKnownKeys(null, ['mode']), {})
  assert.deepEqual(pickKnownKeys(42, ['mode']), {})
})

test('pickStringMap keeps only string values', () => {
  assert.deepEqual(pickStringMap({ Host: 'h', nested: { a: 1 }, n: 2 }), {
    Host: 'h',
  })
  assert.deepEqual(pickStringMap([1, 2]), {})
  assert.deepEqual(pickStringMap('Host'), {})
})

test('pickStringMap keeps a valid string map untouched', () => {
  assert.deepEqual(pickStringMap({ Host: 'h', 'X-A': 'b' }), {
    Host: 'h',
    'X-A': 'b',
  })
})
