import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')

test('три редактора подписки переключают вид одной парой «Форма | YAML»', () => {
  for (const path of [
    './rules-editor-viewer.tsx',
    './proxies-editor-viewer.tsx',
    './groups-editor-viewer.tsx',
  ]) {
    assert.match(source(path), /<EditorModeSwitch\b/, path)
  }
})

test('уведомление о неразобранном файле называет те же виды', () => {
  for (const lang of ['ru', 'en']) {
    const profiles = JSON.parse(source(`../../locales/${lang}/profiles.json`))
    const shared = JSON.parse(source(`../../locales/${lang}/shared.json`))
    const notice: string = profiles.page.feedback.notifications.editorBrokenYaml
    assert.ok(notice.includes(shared.editorModes.form), lang)
    assert.ok(notice.includes('YAML'), lang)
  }
})
