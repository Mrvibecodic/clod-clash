import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')
const ru = (ns: string) => JSON.parse(source(`../../locales/ru/${ns}.json`))

test('русские тексты Настроек, «Логов» и уведомлений — по-русски и одним словом', () => {
  const shared = ru('shared')
  for (const text of Object.values<string>(shared.feedback.validation.merge)) {
    assert.doesNotMatch(text, /Merge File/)
  }
  const settings = ru('settings')
  assert.doesNotMatch(settings.components.verge.basic.fields.copyEnvType, /Env/)
  for (const text of Object.values<string>(settings.modals.misc.fields)) {
    assert.doesNotMatch(text, /лог/i)
  }
  assert.doesNotMatch(shared.filters.logLevels.all, /[A-Z]{3}/)
})

test('уровни в фильтре «Логов» — те же слова, что в Настройках', () => {
  const logs = source('../../pages/logs.tsx')
  assert.doesNotMatch(
    logs,
    /shared\.filters\.logLevels\.(debug|info|warn|error)/,
  )
  for (const level of ['debug', 'info', 'warning', 'error']) {
    assert.match(
      logs,
      new RegExp(
        `settings\\.sections\\.clash\\.form\\.options\\.logLevel\\.${level}`,
      ),
    )
  }
})

test('пустая запись веб-интерфейса не сохраняется и не показывается словом NULL', () => {
  const item = source('./mods/web-ui-item.tsx')
  assert.doesNotMatch(item, /'NULL'/)
  assert.match(item, /disabled=\{!editValue\?\.trim\(\)\}/)
})
