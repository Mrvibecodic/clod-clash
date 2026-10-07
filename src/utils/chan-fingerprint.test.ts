import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { chanFingerprint } from './chan-fingerprint.ts'

describe('отпечаток ключа прослойки', () => {
  it('совпадает с тем, что показывает админка провайдера', async () => {
    // Ключ и отпечаток из общих векторов протокола (chan_vectors.json).
    assert.equal(
      await chanFingerprint('pOCSkrZRwni5dyxWn1-puxPZBrRqtoyd-dwrRAn4ogk'),
      'GpLyOF',
    )
  })

  it('неразборчивый ключ отпечатка не даёт', async () => {
    assert.equal(await chanFingerprint('не ключ'), undefined)
    assert.equal(await chanFingerprint('AAAA'), undefined)
  })
})
