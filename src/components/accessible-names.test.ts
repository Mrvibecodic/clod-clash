import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')

test('переключатели поиска — кнопки с признаком «нажато» и переведённым именем', () => {
  const search = source('./base/base-search-box.tsx')
  assert.doesNotMatch(search, /'active' : 'inactive'/)
  for (const key of ['matchCase', 'matchWholeWord', 'useRegex']) {
    assert.match(
      search,
      new RegExp(
        `<IconButton[^>]*aria-label=\\{t\\('shared\\.placeholders\\.${key}'\\)\\}[^>]*aria-pressed=`,
      ),
      key,
    )
  }
})

test('кнопка строки в редакторах узлов и групп названа, как в редакторе правил', () => {
  for (const path of ['./profile/proxy-item.tsx', './profile/group-item.tsx']) {
    assert.match(
      source(path),
      /<IconButton aria-label=\{actionLabel\} onClick=\{onDelete\}>/,
      path,
    )
  }
})

test('поле сочетания названо действием', () => {
  assert.match(
    source('./setting/mods/hotkey-input.tsx'),
    /aria-label=\{label\}/,
  )
  assert.match(
    source('./setting/mods/hotkey-viewer.tsx'),
    /label=\{t\(HOTKEY_FUNC_LABELS\[func\]\)\}/,
  )
})
