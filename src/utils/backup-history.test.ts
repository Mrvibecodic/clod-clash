import assert from 'node:assert/strict'
import { test } from 'node:test'

import { backupHistoryNote } from './backup-history.ts'

test('«копий нет» — только когда список получен и пуст, один раз', () => {
  assert.equal(backupHistoryNote({ listed: true, total: 0 }), 'empty')
  assert.equal(backupHistoryNote({ listed: false, total: 0 }), null)
  assert.equal(backupHistoryNote({ listed: true, total: 3 }), 'summary')
})
