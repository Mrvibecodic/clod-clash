import assert from 'node:assert/strict'
import { test } from 'node:test'

import { isNetwork, isPortList, normalizeNetwork } from './rule-values.ts'

test('порт правила: один, диапазон и списки, как понимает ядро', () => {
  for (const value of ['80', '8000-9000', '80/443', '80,443', '80/8000-9000']) {
    assert.equal(isPortList(value), true, value)
  }
})

test('порт правила: мусор и выход за 1…65535 не проходят', () => {
  for (const value of [
    '',
    '0',
    '65536',
    '80-',
    '-80',
    '1-2-3',
    'http',
    '80,',
  ]) {
    assert.equal(isPortList(value), false, value)
  }
})

test('сеть правила — в любом регистре, в правило уходит строчными', () => {
  assert.equal(isNetwork('UDP'), true)
  assert.equal(isNetwork('tcp'), true)
  assert.equal(isNetwork('icmp'), false)
  assert.equal(normalizeNetwork('UDP'), 'udp')
})
