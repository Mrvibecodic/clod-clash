import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')

const count = (text: string, pattern: RegExp) =>
  [...text.matchAll(pattern)].length

test('редактор групп читает файлы при открытии и после обновления подписки, а не на каждую правку списка', () => {
  const groups = source('./groups-editor-viewer.tsx')
  assert.equal(count(groups, /readEachFile\(/g), 1)
  assert.match(
    groups,
    /\}, \[open, profileUid, mergeUid, proxiesUid, updated\]\)/,
  )
  assert.equal(count(groups, /parseYamlSafe\(data\)/g), 1)
  assert.match(
    source('./profile-item.tsx'),
    /<GroupsEditorViewer[^>]*\bupdated=\{updated\}/s,
  )
})

test('подписку и merge редактор правил разбирает по разу, файл правок — один разбор', () => {
  const rules = source('./rules-editor-viewer.tsx')
  assert.equal(count(rules, /parseYamlSafe\(data\)/g), 1)
  assert.equal(count(rules, /parseYamlSafe\(mergeData\)/g), 1)
  const seq = source('../../hooks/use-seq-document.ts')
  assert.equal(count(seq, /readSeqConfig\(/g), 1)
})
