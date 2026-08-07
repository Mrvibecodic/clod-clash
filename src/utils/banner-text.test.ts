/// <reference types="node" />
// clod:tests — типы Node подключаем ТОЛЬКО здесь, директивой. В `tsconfig`
// стоит явный список `types`, и добавление в него «node» дало бы node-глобалы
// всему фронту: `setTimeout` начал бы возвращать `NodeJS.Timeout` вместо
// `number` и переломал бы таймеры в компонентах.

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { parseBannerText } from './banner-text.ts'

/** Текст и цвет каждого куска — то, что реально видит человек. */
const shape = (text: string) =>
  parseBannerText(text).map(({ text: body, color }) => [body, color] as const)

describe('parseBannerText', () => {
  it('красит слово после маркера и гасит цвет на пробеле', () => {
    assert.deepEqual(shape('#EF4444ВАЖНО: текст'), [
      ['ВАЖНО:', '#EF4444'],
      [' текст', undefined],
    ])
  })

  it('маркер перед пробелом — обычный текст', () => {
    // Иначе провайдер, написавший решётку с кодом просто так, получил бы
    // невидимую дыру в объявлении.
    assert.deepEqual(shape('#EF4444 текст'), [['#EF4444 текст', undefined]])
  })

  it('не шесть шестнадцатеричных — не маркер', () => {
    assert.deepEqual(shape('#12 руб'), [['#12 руб', undefined]])
    assert.deepEqual(shape('#XYZXYZтекст'), [['#XYZXYZтекст', undefined]])
    assert.deepEqual(shape('#'), [['#', undefined]])
  })

  it('цепочка маркеров съедается целиком, красит последний', () => {
    // Правило то же, что в Rust у счётчика видимых символов: маркер —
    // форматирование нулевой ширины, и в лимит 500 он не входит.
    assert.deepEqual(shape('#AAAAAA#BBBBBBслово'), [['слово', '#BBBBBB']])
  })

  it('маркер в конце строки ничего не красит', () => {
    // Красить нечего: за маркером не осталось ни одного символа.
    assert.deepEqual(shape('текст #EF4444'), [['текст #EF4444', undefined]])
  })

  it('несколько цветных слов в одной строке', () => {
    assert.deepEqual(shape('#FF0000раз #00FF00два'), [
      ['раз', '#FF0000'],
      [' ', undefined],
      ['два', '#00FF00'],
    ])
  })

  it('пустой текст не даёт кусков', () => {
    assert.deepEqual(shape(''), [])
  })

  it('начало куска указывает на исходный текст', () => {
    // `start` — ключ для React, он обязан быть уникальным и расти.
    const starts = parseBannerText('#FF0000раз #00FF00два').map((f) => f.start)
    assert.deepEqual(
      starts,
      [...starts].sort((a, b) => a - b),
    )
    assert.equal(new Set(starts).size, starts.length)
  })
})
