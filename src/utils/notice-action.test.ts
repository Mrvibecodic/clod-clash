import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { runNoticeAction } from './notice-action.ts'

const outcome = async (act: () => unknown) => {
  const seen: string[] = []
  await runNoticeAction(
    act,
    () => seen.push('done'),
    (err) => seen.push(`failed: ${String(err)}`),
  )
  return seen
}

describe('действие из уведомления', () => {
  it('удалось — уведомление закрывается', async () => {
    assert.deepEqual(await outcome(() => undefined), ['done'])
    assert.deepEqual(await outcome(() => Promise.resolve()), ['done'])
  })

  it('отказ — ошибка с причиной, уведомление остаётся', async () => {
    assert.deepEqual(await outcome(() => Promise.reject('refused')), [
      'failed: refused',
    ])
  })

  it('исключение в самом действии — тоже ошибка, а не тишина', async () => {
    assert.deepEqual(
      await outcome(() => {
        throw 'thrown'
      }),
      ['failed: thrown'],
    )
  })
})
