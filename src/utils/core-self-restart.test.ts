import assert from 'node:assert/strict'
import { test } from 'node:test'

import { coreRestartsCleanly } from './core-self-restart.ts'

test('Clod Core с выходом вместо копии узнаётся по версии', () => {
  assert.equal(coreRestartsCleanly('v1.19.31-clod.8'), true)
  assert.equal(coreRestartsCleanly('v1.19.31-clod.12'), true)
  assert.equal(coreRestartsCleanly('v1.19.32-clod.1'), true)
  assert.equal(coreRestartsCleanly('v1.20.0-clod.1'), true)
})

test('прежние сборки Clod Core и стоковый Mihomo — нет', () => {
  assert.equal(coreRestartsCleanly('v1.19.31-clod.7'), false)
  assert.equal(coreRestartsCleanly('v1.19.30-clod.9'), false)
  assert.equal(coreRestartsCleanly('v1.19.31'), false)
  assert.equal(coreRestartsCleanly('alpha-1a2b3c4'), false)
  assert.equal(coreRestartsCleanly('v1.19.31-clod-61af9c3'), false)
  assert.equal(coreRestartsCleanly(undefined), false)
  assert.equal(coreRestartsCleanly(''), false)
})
