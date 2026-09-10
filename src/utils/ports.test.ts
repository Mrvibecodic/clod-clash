import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  findDuplicatePort,
  findPortOutOfRange,
  MAX_PORT,
  MIN_PORT,
  portRangeVerdict,
} from './ports.ts'

test('находит первый повтор и называет именно его', () => {
  assert.equal(findDuplicatePort([7897, 7898, 7899, 7898]), 7898)
  assert.equal(findDuplicatePort([7897, 7897]), 7897)
})

test('без повторов ничего не находит', () => {
  assert.equal(findDuplicatePort([]), undefined)
  assert.equal(findDuplicatePort([7897]), undefined)
  assert.equal(findDuplicatePort([7897, 7898, 7899]), undefined)
})

test('порог порта один и тот же для каждой границы', () => {
  assert.equal(MIN_PORT, 1000)
  assert.equal(MAX_PORT, 65535)
  assert.equal(portRangeVerdict(999), 'tooLow')
  assert.equal(portRangeVerdict(1), 'tooLow')
  assert.equal(portRangeVerdict(MIN_PORT), 'ok')
  assert.equal(portRangeVerdict(7897), 'ok')
  assert.equal(portRangeVerdict(MAX_PORT), 'ok')
  assert.equal(portRangeVerdict(65536), 'tooHigh')
})

test('называет первый негодный порт полезной нагрузки и чем он плох', () => {
  assert.deepEqual(findPortOutOfRange([7897, 800, 70000]), {
    port: 800,
    verdict: 'tooLow',
  })
  assert.deepEqual(findPortOutOfRange([7897, 70000]), {
    port: 70000,
    verdict: 'tooHigh',
  })
})

test('ноль — это «слушателя нет», а не негодный порт', () => {
  assert.equal(findPortOutOfRange([0, 0, 0]), undefined)
  assert.equal(findPortOutOfRange([]), undefined)
  assert.equal(findPortOutOfRange([7897, 7898, 7899]), undefined)
  assert.deepEqual(findPortOutOfRange([0, 800]), {
    port: 800,
    verdict: 'tooLow',
  })
})
