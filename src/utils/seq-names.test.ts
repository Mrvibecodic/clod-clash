import assert from 'node:assert/strict'
import { test } from 'node:test'

import { seqNameConflict, takenSeqNames } from './seq-names.ts'

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
  const taken = takenSeqNames(named(), named('Авто'), named())
  assert.equal(seqNameConflict('NL-01', 'group', taken, ['NL-01']), 'proxy')
  assert.equal(seqNameConflict('Авто', 'group', taken, ['NL-01']), 'taken')
  assert.equal(seqNameConflict('Видео', 'group', taken, ['NL-01']), null)
})

test('встроенные имена ядра не даются ни группе, ни прокси', () => {
  const taken = takenSeqNames(named())
  for (const name of [
    'DIRECT',
    'REJECT',
    'REJECT-DROP',
    'PASS',
    'PASS-RULE',
    'COMPATIBLE',
  ]) {
    assert.equal(seqNameConflict(name, 'group', taken), 'builtin', name)
    assert.equal(seqNameConflict(name, 'proxy', taken), 'builtin', name)
  }
  assert.equal(seqNameConflict('GLOBAL', 'proxy', taken), 'builtin')
  assert.equal(seqNameConflict('GLOBAL', 'group', taken), null)
  assert.equal(seqNameConflict('direct', 'proxy', taken), null)
})
