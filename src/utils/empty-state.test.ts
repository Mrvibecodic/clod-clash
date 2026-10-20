import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { emptyState } from './empty-state.ts'

const page = { coreDown: false, searching: false, hasData: false }

describe('emptyState', () => {
  it('a stopped core explains the empty screen first', () => {
    assert.equal(
      emptyState({ ...page, coreDown: true, searching: true, hasData: true }),
      'coreDown',
    )
  })

  it('a failed read is not an endless spinner', () => {
    assert.equal(emptyState({ ...page, failed: true, loading: true }), 'failed')
    assert.equal(emptyState({ ...page, loading: true }), 'loading')
  })

  it('a search that found nothing among the data says so', () => {
    assert.equal(
      emptyState({ ...page, searching: true, hasData: true }),
      'notFound',
    )
    assert.equal(emptyState({ ...page, searching: true }), 'empty')
    assert.equal(emptyState({ ...page, hasData: true }), 'empty')
  })

  it('the log level of the core explains missing entries before the search', () => {
    assert.equal(
      emptyState({
        ...page,
        belowCoreLevel: true,
        searching: true,
        hasData: true,
      }),
      'belowCoreLevel',
    )
  })

  it('rules, connections and logs explain their empty screen the same way', () => {
    for (const file of ['rules', 'connections', 'logs']) {
      const source = readFileSync(
        new URL(`../pages/${file}.tsx`, import.meta.url),
        'utf8',
      )
      assert.match(source, /emptyState\(\{/, file)
      assert.match(source, /<CoreStatus \/>/, file)
    }
  })
})
