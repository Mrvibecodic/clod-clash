import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

const source = (path: string) =>
  readFileSync(new URL(path, import.meta.url), 'utf8')

describe('кнопки «Обновить подписку»', () => {
  it('все идут через один хук и одно состояние «идёт обновление»', () => {
    for (const path of [
      '../components/home/provider-header.tsx',
      '../components/home/no-servers-status.tsx',
      '../hooks/use-traffic-estimate.ts',
      '../pages/home-advanced.tsx',
    ]) {
      const text = source(path)
      assert.match(text, /useSubscriptionUpdate\(/, path)
      assert.doesNotMatch(text, /updateProfile\(/, path)
    }
  })

  it('на время паузы после обновления все кнопки неактивны', () => {
    for (const path of [
      '../components/home/provider-header.tsx',
      '../components/home/no-servers-status.tsx',
      '../components/home/subscription-card.tsx',
      '../pages/home-advanced.tsx',
    ]) {
      assert.match(source(path), /disabled=\{\w+ \|\| \w*[pP]aused\}/, path)
    }
  })

  it('экран «Подписки» занят по тому же состоянию, своё — только на время вызова', () => {
    const card = source('../components/profile/profile-item.tsx')
    assert.match(card, /useSubscriptionUpdating\(itemData\.uid\)/)
    assert.match(card, /beginOwnUpdate\(itemData\.uid\)/)
    assert.match(card, /finally \{\s+endOwnUpdate\(itemData\.uid\)/)

    const page = source('../pages/profiles.tsx')
    assert.match(page, /isSubscriptionUpdating\(uid\)/)
    assert.match(page, /finally \{\s+endOwnUpdate\(uid\)/)
    assert.doesNotMatch(page + card, /LoadingCache/)
  })
})

describe('снимок бэкенда в окне', () => {
  it('снимок слушает каркас окна, сверка — после того, как слушатель встал', () => {
    assert.match(
      source('../pages/_layout/hooks/use-layout-events.ts'),
      /useSubscriptionUpdateEvents\(\)/,
    )
    const hook = source('./use-subscription-update.ts')
    assert.match(
      hook,
      /useTauriEvent<UpdatesInFlight>\(\s+'clod:\/\/profiles-updating',\s+\(\{ payload \}\) => applySnapshot\(payload\),\s+\(\) => void syncWithBackend\(\),\s+\)/,
    )
    assert.match(
      hook,
      /useTauriEvent\('verge:\/\/window-shown', \(\) => void syncWithBackend\(\)\)/,
    )
  })

  it('сверка и событие применяются одной функцией по номеру снимка', () => {
    const hook = source('./use-subscription-update.ts')
    assert.match(hook, /if \(snapshot\) applySnapshot\(snapshot\)/)
    assert.match(
      hook,
      /if \(updates\.apply\(snapshot\)\.length > 0\) \{\s+void revalidateQueries\(\[\['getProfiles'\]\]\)/,
    )
  })

  it('слушатель зовёт onReady после регистрации, и только живой', () => {
    const listen = source('./use-listen.ts')
    assert.match(
      listen,
      /\.finally\(\(\) => \{\s+if \(!disposed\) onReadyRef\.current\?\.\(\)/,
    )
  })
})
