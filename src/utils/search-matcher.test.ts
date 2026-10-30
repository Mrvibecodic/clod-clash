import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { buildHighlightRegex } from './search-matcher.ts'

const found = (query: string, options: object, text: string) => {
  const regex = buildHighlightRegex(query, options)
  return regex ? [...text.matchAll(regex)].map((match) => match[0]) : null
}

describe('подсветка найденного в журнале', () => {
  it('пустой поиск ничего не подсвечивает', () => {
    assert.equal(buildHighlightRegex(''), null)
    assert.equal(buildHighlightRegex('   '), null)
  })

  it('текст ищется как есть, без учёта регистра', () => {
    assert.deepEqual(found('a.b', {}, 'A.B axb a.b'), ['A.B', 'a.b'])
  })

  it('с учётом регистра — только точное', () => {
    assert.deepEqual(found('Dns', { matchCase: true }, 'dns Dns DNS'), ['Dns'])
  })

  it('целое слово — без частей других слов', () => {
    assert.deepEqual(found('tun', { matchWholeWord: true }, 'tun tunnel tun'), [
      'tun',
      'tun',
    ])
  })

  it('регекс подсвечивает совпадения, кривой — как обычный текст', () => {
    assert.deepEqual(found('\\d+', { useRegularExpression: true }, 'a1 b22'), [
      '1',
      '22',
    ])
    assert.deepEqual(found('a(', { useRegularExpression: true }, 'xa(y'), [
      'a(',
    ])
  })

  it('один регекс годится для любого числа строк подряд', () => {
    const regex = buildHighlightRegex('x')
    assert.ok(regex)
    if (!regex) return
    assert.equal([...'x x'.matchAll(regex)].length, 2)
    assert.equal([...'x x'.matchAll(regex)].length, 2)
    assert.equal(regex.lastIndex, 0)
  })
})
