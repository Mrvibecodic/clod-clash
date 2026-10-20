import assert from 'node:assert/strict'
import { test } from 'node:test'

import { backupHistoryNote } from './backup-history.ts'

const base = { notConfigured: false, serverFailed: false, listed: true }

test('сервер WebDAV не ответил — только это, без «копий нет»', () => {
  assert.equal(
    backupHistoryNote({ ...base, serverFailed: true, listed: false, total: 0 }),
    'serverFailed',
  )
})

test('WebDAV не настроен — только это', () => {
  assert.equal(
    backupHistoryNote({
      ...base,
      notConfigured: true,
      listed: false,
      total: 0,
    }),
    'notConfigured',
  )
})

test('«копий нет» — только когда список получен и пуст, один раз', () => {
  assert.equal(backupHistoryNote({ ...base, total: 0 }), 'empty')
  assert.equal(backupHistoryNote({ ...base, listed: false, total: 0 }), null)
  assert.equal(backupHistoryNote({ ...base, total: 3 }), 'summary')
})
