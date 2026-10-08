import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { profileDate, refillDateText, sessionTimeText } from './date-text.ts'

// Полдень по UTC: в любом поясе от −11 до +11 это тот же день.
const NOON = Date.UTC(2027, 1, 3, 12) / 1000

describe('даты подписки', () => {
  it('экран профилей пишет YYYY-MM-DD, без даты — прочерк', () => {
    assert.equal(profileDate(NOON), '2027-02-03')
    assert.equal(profileDate(undefined), '-')
    assert.equal(profileDate(0), '-')
  })

  it('дата пополнения на Главной — DD.MM.YYYY, без даты — ничего', () => {
    const profile = (refill_date?: number) =>
      ({ uid: 'test', type: 'remote', refill_date }) as IProfileItem
    assert.equal(refillDateText(profile(NOON)), '03.02.2027')
    assert.equal(refillDateText(profile(undefined)), undefined)
    assert.equal(refillDateText(undefined), undefined)
  })
})

describe('таймер сессии на кнопке подключения', () => {
  it('как на Android: минуты двумя цифрами, часы без нуля впереди', () => {
    assert.equal(sessionTimeText(1), '00:01')
    assert.equal(sessionTimeText(65.9), '01:05')
    assert.equal(sessionTimeText(3599), '59:59')
    assert.equal(sessionTimeText(3600), '1:00:00')
    assert.equal(sessionTimeText(36_000 + 62), '10:01:02')
  })

  it('отрицательное (часы устройства отстали) — ноль', () => {
    assert.equal(sessionTimeText(-5), '00:00')
  })
})
