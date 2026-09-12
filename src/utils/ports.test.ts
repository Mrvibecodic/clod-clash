import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  findDuplicatePort,
  findPortOutOfRange,
  isProxyServerAt,
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

test('адрес IPv6 узнаётся и со скобками, и без них', () => {
  assert.equal(isProxyServerAt('::1:7897', '[::1]', 7897), true)
  assert.equal(isProxyServerAt('[::1]:7897', '::1', 7897), true)
  assert.equal(isProxyServerAt('::1:7897', '[::1]', 7890), false)
  assert.equal(isProxyServerAt('::2:7897', '[::1]', 7897), false)
})

test('обычный адрес сравнивается по хосту и порту по отдельности', () => {
  assert.equal(isProxyServerAt('127.0.0.1:7897', '127.0.0.1', 7897), true)
  assert.equal(isProxyServerAt('127.0.0.1:7897', '127.0.0.1', 7898), false)
  assert.equal(isProxyServerAt('127.0.0.1:78970', '127.0.0.1', 7897), false)
})

test('без адреса или без действующего порта совпадения нет', () => {
  assert.equal(isProxyServerAt(undefined, '127.0.0.1', 7897), false)
  assert.equal(isProxyServerAt('', '127.0.0.1', 7897), false)
  assert.equal(isProxyServerAt(':7897', '127.0.0.1', 7897), false)
  assert.equal(isProxyServerAt('127.0.0.1:7897', '127.0.0.1', undefined), false)
})
