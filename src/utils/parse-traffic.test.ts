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

  // Те же примеры — в src-tauri/src/utils/speed.rs: строка меню macOS пишет
  // скорость так же, как окно.
  it('скорость — как в строке меню', () => {
    const cases: [number, string][] = [
      [0, '0.00 B/s'],
      [5, '5.00 B/s'],
      [512, '512 B/s'],
      [999, '999 B/s'],
      [1000, '0.977 KB/s'],
      [1023, '0.999 KB/s'],
      [1024, '1.00 KB/s'],
      [1152, '1.13 KB/s'],
      [10_235, '10.0 KB/s'],
      [102_348, '99.9 KB/s'],
      [1_023_385, '999 KB/s'],
      [1_023_590, '0.976 MB/s'],
      [1_042_432, '0.994 MB/s'],
      [1_258_291, '1.20 MB/s'],
      [13_314_398_618, '12.4 GB/s'],
    ]
    for (const [bytes, shown] of cases) assert.equal(parseSpeed(bytes), shown)
    setTrafficLocale('ru')
    assert.equal(parseSpeed(1_258_291), '1,20 MB/s')
    assert.equal(parseSpeed(512), '512 B/s')
  })

  it('разделитель дробной части — по языку, единицы английские', () => {
    setTrafficLocale('ru')
    assert.equal(parseTraffic(1024 ** 3 * 12.4), '12,4 GB')
    assert.equal(parseTraffic(512), '512 B')
    setTrafficLocale('en')
    assert.equal(parseTraffic(1024 ** 3 * 12.4), '12.4 GB')
  })
})
