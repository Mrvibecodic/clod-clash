import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { localeTag, resolveLanguage } from './language.ts'

describe('язык интерфейса по языку системы', () => {
  it('не из списка — английский; языки бывшего СССР — русский', () => {
    const table: [string | undefined, string][] = [
      ['pl-PL', 'en'],
      ['fr', 'en'],
      [undefined, 'en'],
      ['uk-UA', 'ru'],
      ['be', 'ru'],
      ['kk-KZ', 'ru'],
      ['uz_UZ', 'ru'],
      ['ky', 'ru'],
      ['tg', 'ru'],
      ['ru-RU', 'ru'],
      ['zh', 'zh'],
      ['zh-TW', 'zhtw'],
      ['de-DE', 'de'],
      // Как у трея (Rust): японский и традиционный китайский по любой записи.
      ['ja', 'jp'],
      ['ja-JP', 'jp'],
      ['zh-HK', 'zhtw'],
      ['zh-Hant', 'zhtw'],
      ['zh-Hant-HK', 'zhtw'],
      ['zh-Hans-CN', 'zh'],
      ['zh-SG', 'zh'],
      ['jp', 'jp'],
      ['zhtw', 'zhtw'],
    ]
    for (const [system, language] of table) {
      assert.equal(resolveLanguage(system), language, String(system))
    }
  })

  it('страница получает метку BCP-47, а не внутренний код', () => {
    assert.equal(localeTag('jp'), 'ja')
    assert.equal(localeTag('zhtw'), 'zh-TW')
    assert.equal(localeTag('ru'), 'ru')
  })
})
