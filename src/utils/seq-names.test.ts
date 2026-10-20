import assert from 'node:assert/strict'
import { test } from 'node:test'

import { takenSeqNames } from './seq-names.ts'

const named = (...names: string[]) => names.map((name) => ({ name }))

test('имя, добавленное в начало, занято и для добавления в конец', () => {
  const taken = takenSeqNames(named('Видео'), named('Авто'), named())
  assert.equal(taken.has('Видео'), true)
})

test('имя, добавленное в конец, занято и для добавления в начало', () => {
  const taken = takenSeqNames(named(), named('Авто'), named('Видео'))
  assert.equal(taken.has('Видео'), true)
  assert.equal(taken.has('Авто'), true)
  assert.equal(taken.has('Новое'), false)
})
