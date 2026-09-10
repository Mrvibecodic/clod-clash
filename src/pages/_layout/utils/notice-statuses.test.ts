import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { NOTICE_STATUSES, isNoticeStatus } from './notice-statuses.ts'

const backendSource = readFileSync(
  new URL('../../../../src-tauri/src/core/notification.rs', import.meta.url),
  'utf8',
)

const declaredByBackend = (): string[] => {
  const block = /NOTICE_STATUSES: &\[&str\] = &\[([^\]]*)\]/.exec(backendSource)
  assert.ok(block, 'в notification.rs нет списка NOTICE_STATUSES')
  return [...block[1].matchAll(/"([^"]+)"/g)].map((match) => match[1])
}

describe('статусы уведомлений', () => {
  it('каждый статус, который шлёт бэкенд, известен странице', () => {
    const backend = declaredByBackend()
    assert.ok(backend.length > 0)
    for (const status of backend) {
      assert.ok(isNoticeStatus(status), `у статуса ${status} нет обработчика`)
    }
  })

  it('страница не ждёт статусов, которых бэкенд не шлёт', () => {
    const backend = new Set(declaredByBackend())
    for (const status of NOTICE_STATUSES) {
      assert.ok(backend.has(status), `статус ${status} бэкенд не шлёт`)
    }
  })

  it('список без повторов', () => {
    assert.equal(new Set(NOTICE_STATUSES).size, NOTICE_STATUSES.length)
  })
})
