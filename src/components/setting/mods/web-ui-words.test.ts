import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

test('окно «Веб-интерфейс» называет себя и раздел Настроек их словами', () => {
  for (const lang of ['ru', 'en']) {
    const settings = JSON.parse(
      readFileSync(
        new URL(`../../../locales/${lang}/settings.json`, import.meta.url),
        'utf8',
      ),
    )
    const { needsController, noController } = settings.modals.webUI.messages
    assert.ok(
      needsController
        .toLowerCase()
        .includes(settings.modals.webUI.title.toLowerCase()),
      lang,
    )
    assert.match(noController, /Clash/, lang)
    assert.doesNotMatch(noController, /core settings|настройках ядра/, lang)
  }
})
