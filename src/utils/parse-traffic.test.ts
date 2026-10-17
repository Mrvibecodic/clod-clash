import assert from 'node:assert/strict'
import { afterEach, describe, it } from 'node:test'

import parseTraffic, { parseSpeed, setTrafficLocale } from './parse-traffic.ts'

describe('разбор трафика', () => {
  afterEach(() => setTrafficLocale('en'))

  it('от тысячи — следующая единица, а не четыре цифры', () => {
    assert.equal(parseTraffic(1024 * 1018), '0.994 MB')
    assert.equal(parseTraffic(1024 * 999.6), '0.976 MB')
    assert.equal(parseSpeed(1000), '0.977 KB/s')
  })

  it('ниже тысячи остаётся три значащие цифры', () => {
    assert.equal(parseTraffic(1024 * 999.4), '999 KB')
    assert.equal(parseTraffic(1024 ** 3 * 12.4), '12.4 GB')
  })

  it('разделитель дробной части — по языку, единицы английские', () => {
    setTrafficLocale('ru')
    assert.equal(parseTraffic(1024 ** 3 * 12.4), '12,4 GB')
    assert.equal(parseTraffic(512), '512 B')
    setTrafficLocale('en')
    assert.equal(parseTraffic(1024 ** 3 * 12.4), '12.4 GB')
  })
})
