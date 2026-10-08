import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { prepareTunWith } from './prepare-tun.ts'

describe('подготовка TUN', () => {
  it('отказ подготовки уходит вызывающему, состояние перечитано', async () => {
    let refreshed = 0
    const refused = new Error('clod-tun-setup-busy')
    await assert.rejects(
      prepareTunWith(
        () => Promise.reject(refused),
        async () => {
          refreshed += 1
        },
      ),
      refused,
    )
    assert.equal(refreshed, 1)
  })

  it('человек отказал — false, состояние перечитано', async () => {
    let refreshed = 0
    const ready = await prepareTunWith(
      async () => false,
      async () => {
        refreshed += 1
      },
    )
    assert.equal(ready, false)
    assert.equal(refreshed, 1)
  })

  it('сбой перечитывания не делает удачную подготовку неудачной', async () => {
    const ready = await prepareTunWith(
      async () => true,
      () => Promise.reject(new Error('no window')),
    )
    assert.equal(ready, true)
  })
})
