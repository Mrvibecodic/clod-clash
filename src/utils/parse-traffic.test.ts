import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import parseTraffic from './parse-traffic.ts'

describe('разбор трафика', () => {
  it('у тысячи не уходит в экспоненциальную запись', () => {
    assert.deepEqual(parseTraffic(1024 * 999.6), ['1000', 'KB'])
  })

  it('ниже границы округления остаётся три значащие цифры', () => {
    assert.deepEqual(parseTraffic(1024 * 999.4), ['999', 'KB'])
  })
})
