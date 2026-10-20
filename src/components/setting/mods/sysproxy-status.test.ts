import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

test('«Текущий системный прокси» берёт режим из сохранённой настройки, а не из формы', () => {
  const viewer = readFileSync(
    new URL('./sysproxy-viewer.tsx', import.meta.url),
    'utf8',
  )
  assert.match(viewer, /\{proxy_auto_config \? PAC_URL : systemProxyAddress\}/)
  assert.doesNotMatch(viewer, /value\.pac \? PAC_URL/)
})
