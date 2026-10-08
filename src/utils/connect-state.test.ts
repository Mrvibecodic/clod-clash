import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { connectState } from './connect-state.ts'

describe('кнопка «Подключить»', () => {
  it('показывает факт, пока ничего не идёт', () => {
    assert.deepEqual(connectState({ busy: false, connected: true }), {
      state: 'on',
      errorText: undefined,
    })
    assert.equal(connectState({ busy: false, connected: false }).state, 'off')
  })

  it('на время нажатия — что делается, без намерения — подключение', () => {
    const busy = { busy: true, connected: false }
    assert.equal(
      connectState({ ...busy, intent: 'disconnecting' }).state,
      'disconnecting',
    )
    assert.equal(connectState(busy).state, 'connecting')
  })

  it('ошибка видна, пока факт тот же, что при провале', () => {
    const failure = { text: 'служба не ответила', at: false }
    assert.deepEqual(connectState({ failure, busy: false, connected: false }), {
      state: 'error',
      errorText: 'служба не ответила',
    })
    // Ошибка важнее занятости: повторное нажатие её не прячет раньше времени.
    assert.equal(
      connectState({ failure, busy: true, connected: false }).state,
      'error',
    )
    // Факт сменился — ошибка устарела.
    assert.deepEqual(connectState({ failure, busy: false, connected: true }), {
      state: 'on',
      errorText: undefined,
    })
  })
})
