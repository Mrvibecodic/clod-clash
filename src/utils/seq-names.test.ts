import assert from 'node:assert/strict'
import { test } from 'node:test'

import { groupNameConflict, takenSeqNames } from './seq-names.ts'

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

test('имя новой группы не может совпадать с именем прокси: у ядра одно пространство имён', () => {
  const groups = [named(), named('Авто'), named()]
  assert.equal(groupNameConflict('NL-01', groups, ['NL-01', 'DE-02']), 'proxy')
  assert.equal(groupNameConflict('Авто', groups, ['NL-01']), 'group')
  assert.equal(groupNameConflict('Видео', groups, ['NL-01']), null)
})
