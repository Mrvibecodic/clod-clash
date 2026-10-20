import assert from 'node:assert/strict'
import { test } from 'node:test'

import { editorSaveBlocked } from './editor-save.ts'

test('файл не прочитался — «Сохранить» недоступно, даже если в редактор что-то вписали', () => {
  assert.equal(editorSaveBlocked(false, { reason: 'denied' }, true), true)
})

test('файл прочитан: сохранять можно только несохранённое', () => {
  assert.equal(editorSaveBlocked(false, null, true), false)
  assert.equal(editorSaveBlocked(false, null, false), true)
  assert.equal(editorSaveBlocked(false, undefined, undefined), false)
  assert.equal(editorSaveBlocked(true, null, true), true)
})
