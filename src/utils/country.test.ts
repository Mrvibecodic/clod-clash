/// <reference types="node" />
// clod:tests — типы Node подключаем ТОЛЬКО здесь, директивой. В `tsconfig`
// стоит явный список `types`, и добавление в него «node» дало бы node-глобалы
// всему фронту: `setTimeout` начал бы возвращать `NodeJS.Timeout` вместо
// `number` и переломал бы таймеры в компонентах.

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { countryFromName, flagSrc, nameWithoutFlag } from './country.ts'

describe('countryFromName', () => {
  it('флаг-эмодзи важнее всего остального', () => {
    assert.equal(countryFromName('🇳🇱 Netherlands 01'), 'nl')
    // Эмодзи побеждает даже противоречащее ему имя: его ставит провайдер
    // осознанно, а имя часто остаётся от шаблона.
    assert.equal(countryFromName('🇩🇪 Amsterdam'), 'de')
  })

  it('человеческие имена на двух языках', () => {
    assert.equal(countryFromName('Netherlands 01'), 'nl')
    assert.equal(countryFromName('Нидерланды 01'), 'nl')
    assert.equal(countryFromName('Amsterdam Premium'), 'nl')
    assert.equal(countryFromName('Germany'), 'de')
  })

  it('США распознаются словом, а не куском строки', () => {
    // «Georgia USA» — это штат, а не страна на Кавказе. Куском строки такое
    // не выразить: «USA» прячется внутри «JerUSAlem».
    assert.equal(countryFromName('Georgia USA'), 'us')
    assert.equal(countryFromName('US East 2'), 'us')
  })

  it('UK — это то, что пишут люди, gb — как называется файл флага', () => {
    assert.equal(countryFromName('UK London'), 'gb')
  })

  it('двухбуквенный токен как последняя попытка', () => {
    assert.equal(countryFromName('NL-01'), 'nl')
    assert.equal(countryFromName('vless DE 04'), 'de')
  })

  it('стоп-слова не превращаются в страны', () => {
    // `ip`, `tv`, `vm`, `go`, `ws`, `v2` — это про технику, а не про страны,
    // но у каждого есть свой ISO-код.
    assert.equal(countryFromName('Server IP 12'), undefined)
    assert.equal(countryFromName('node WS 3'), undefined)
  })

  it('пустое имя и полная неизвестность', () => {
    assert.equal(countryFromName(''), undefined)
    assert.equal(countryFromName('Fast Node 12'), undefined)
  })
})

describe('flagSrc', () => {
  it('известный код и заглушка', () => {
    assert.equal(flagSrc('nl'), '/flags/nl.svg')
    // Ссылки на несуществующий файл быть не должно: пустая картинка в списке
    // хуже нейтрального кружка.
    assert.equal(flagSrc('zz'), '/flags/xx.svg')
    assert.equal(flagSrc(undefined), '/flags/xx.svg')
  })
})

describe('nameWithoutFlag', () => {
  it('снимает эмодзи, когда флаг рисуем сами', () => {
    assert.equal(nameWithoutFlag('🇳🇱 Netherlands 01'), 'Netherlands 01')
    assert.equal(nameWithoutFlag('Netherlands 01'), 'Netherlands 01')
  })

  it('имя из одного флага остаётся собой', () => {
    // Иначе строка в списке стала бы пустой.
    assert.equal(nameWithoutFlag('🇳🇱'), '🇳🇱')
  })
})
