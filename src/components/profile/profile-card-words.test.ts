import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')

test('истёкшая подписка на карточке — только дата, без «0 дн»', () => {
  assert.match(
    source('./profile-item.tsx'),
    /daysLeft !== undefined && daysLeft > 0\s*\? t\('profiles\.components\.profileItem\.labels\.expiresIn'/,
  )
})

test('пункт меню назван так же, как окно, которое он открывает', () => {
  for (const lang of ['ru', 'en']) {
    const profiles = JSON.parse(source(`../../locales/${lang}/profiles.json`))
    assert.equal(
      profiles.components.menu.editInfo,
      profiles.modals.profileForm.title.edit,
      lang,
    )
  }
})
