import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { getCacheData, setCacheDataAsync } from './query-client.ts'

describe('кэш запросов', () => {
  it('записанное правкой читается и после сотен других ключей', async () => {
    await setCacheDataAsync(['getRuntimeConfig'], { mode: 'rule' })
    for (let i = 0; i < 300; i++) {
      await setCacheDataAsync(
        ['icon-cache', `https://example.com/${i}.png`, 'k'],
        'x',
      )
    }
    assert.deepEqual(getCacheData(['getRuntimeConfig']), { mode: 'rule' })
  })

  it('правка сливается с тем, что лежит в кэше', async () => {
    await setCacheDataAsync(['getCoreLadder'], { mixed_port: 7897 })
    await setCacheDataAsync<{ mixed_port?: number; log_level?: string }>(
      ['getCoreLadder'],
      (old) => ({ ...old, log_level: 'info' }),
    )
    assert.deepEqual(getCacheData(['getCoreLadder']), {
      mixed_port: 7897,
      log_level: 'info',
    })
  })
})
