import assert from 'node:assert/strict'
import { test } from 'node:test'

import { EMPTY_SEQ_LISTS, seqUnsaved } from './seq-unsaved.ts'

const lists = (prepend: string[]) => ({ prepend, append: [], delete: [] })

test('файл открыт текстом, починен и переведён в наглядный вид — это несохранённое', () => {
  assert.equal(
    seqUnsaved(true, null, lists(['DOMAIN,example.com,DIRECT'])),
    true,
  )
  assert.equal(seqUnsaved(true, null, EMPTY_SEQ_LISTS), true)
})

test('пока файл не прочитан или не прочитался — несохранённого нет', () => {
  assert.equal(seqUnsaved(false, null, EMPTY_SEQ_LISTS), false)
})

test('файл разложился по спискам — сравнение с диском', () => {
  const disk = lists(['DOMAIN,example.com,DIRECT'])
  assert.equal(
    seqUnsaved(true, disk, lists(['DOMAIN,example.com,DIRECT'])),
    false,
  )
  assert.equal(seqUnsaved(true, disk, EMPTY_SEQ_LISTS), true)
})
