import assert from 'node:assert/strict'
import { test } from 'node:test'

import { withListDraft } from './list-draft.ts'

test('набранная, но не добавленная строка входит в сохраняемый список', () => {
  assert.equal(
    withListDraft('localhost;127.*', ' example.com ', ';'),
    'localhost;127.*;example.com',
  )
  assert.equal(withListDraft('', '10.0.0.0/8', ','), '10.0.0.0/8')
  assert.equal(withListDraft(undefined, 'example.com', ','), 'example.com')
})

test('пустая строка ввода ничего не меняет, даже незаданное', () => {
  assert.equal(withListDraft('a,b', '  ', ','), 'a,b')
  assert.equal(withListDraft(undefined, '', ','), undefined)
})
