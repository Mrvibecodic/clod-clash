import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { delayBars, delayColor, delayTone } from './delay-color.ts'

describe('цвет пинга', () => {
  it('без границ подписки остаются 200/400', () => {
    assert.equal(delayTone(199), 'success')
    assert.equal(delayTone(200), 'warning')
    assert.equal(delayTone(399), 'warning')
    assert.equal(delayTone(400), 'error')
    assert.equal(delayTone(0), 'error')
    assert.equal(delayTone(-1), undefined)
    assert.equal(delayTone(undefined), undefined)
    assert.equal(delayTone(300, undefined), 'warning')

    assert.deepEqual(
      [99, 100, 199, 200, 399, 400].map((delay) => delayBars(delay)),
      [4, 3, 3, 2, 2, 1],
    )
    assert.equal(delayColor(150), 'success.main')
    assert.equal(delayColor(undefined), 'text.disabled')
  })

  it('границы подписки сдвигают цвет и полоски', () => {
    const bounds = [100, 150] as const
    assert.equal(delayTone(99, bounds), 'success')
    assert.equal(delayTone(100, bounds), 'warning')
    assert.equal(delayTone(149, bounds), 'warning')
    assert.equal(delayTone(150, bounds), 'error')
    assert.equal(delayTone(0, bounds), 'error')
    assert.equal(delayTone(-1, bounds), undefined)

    assert.deepEqual(
      [49, 50, 99, 100, 149, 150].map((delay) => delayBars(delay, bounds)),
      [4, 3, 3, 2, 2, 1],
    )
    assert.equal(delayColor(120, bounds), 'warning.main')
    assert.equal(delayColor(500, [1000, 2000]), 'success.main')
  })
})
