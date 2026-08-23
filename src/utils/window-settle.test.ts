import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { createStartupSettle } from './window-settle.ts'

describe('createStartupSettle', () => {
  it('пока первый подгон не случился, старт не считается законченным', () => {
    const settle = createStartupSettle(0)
    assert.equal(settle.isGrace(0), true)
    assert.equal(settle.isGrace(30_000), true)
    assert.equal(settle.isGrace(59_999), true)
  })

  it('запуск, в котором подгона так и не было, отпускается по потолку', () => {
    const settle = createStartupSettle(0)
    assert.equal(settle.isGrace(60_000), false)
  })

  it('медленный старт держится, пока первый подгон не устоится', () => {
    const settle = createStartupSettle(0)
    settle.markFitAttempt(45_000)
    assert.equal(settle.isGrace(46_000), true)
    assert.equal(settle.isGrace(49_999), true)
    assert.equal(settle.isGrace(50_000), false)
  })

  it('запоминается только первый подгон', () => {
    const settle = createStartupSettle(0)
    settle.markFitAttempt(1000)
    settle.markFitAttempt(9000)
    assert.equal(settle.isGrace(5_999), true)
    assert.equal(settle.isGrace(6_000), false)
  })

  it('окно пошло за подгоном — старт закончен сразу', () => {
    const settle = createStartupSettle(0)
    settle.markFitAttempt(1000)
    settle.markSettled()
    assert.equal(settle.isGrace(1001), false)
  })
})
