import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  findDuplicatePort,
  findPortOutOfRange,
  MAX_PORT,
  MIN_PORT,
  portRangeVerdict,
  reachableProxyHost,
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

test('пустое поле (ноль) — негодный порт', () => {
  assert.equal(findPortOutOfRange([]), undefined)
  assert.equal(findPortOutOfRange([7897, 7898, 7899]), undefined)
  assert.deepEqual(findPortOutOfRange([7897, 0]), {
    port: 0,
    verdict: 'tooLow',
  })
})

test('нечисловое значение поля — негодный порт, а не пропуск', () => {
  assert.deepEqual(findPortOutOfRange([Number.NaN]), {
    port: Number.NaN,
    verdict: 'tooLow',
  })
  assert.deepEqual(findPortOutOfRange([7897, Number.NaN, 70000]), {
    port: Number.NaN,
    verdict: 'tooLow',
  })
})

test('внешний хост прокси работает только при раздаче в локальную сеть', () => {
  assert.equal(reachableProxyHost('192.168.1.5', false), '127.0.0.1')
  assert.equal(reachableProxyHost('192.168.1.5', true), '192.168.1.5')
  assert.equal(reachableProxyHost('localhost', false), '127.0.0.1')
  assert.equal(reachableProxyHost('localhost', true), 'localhost')
  assert.equal(reachableProxyHost(undefined, false), '127.0.0.1')
})
