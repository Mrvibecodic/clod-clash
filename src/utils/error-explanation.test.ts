import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  RAW_TAIL_LIMIT,
  explainErrorKey,
  trimRawError,
} from './error-explanation.ts'

const key = (raw: string) => explainErrorKey(raw)?.split('.').pop()

describe('explainErrorKey', () => {
  it('узнаёт то, на что жалуется ядро чаще всего', () => {
    assert.equal(
      key('dial tcp: lookup panel.example on 1.1.1.1:53: no such host'),
      'noSuchHost',
    )
    assert.equal(
      key('dial tcp 10.0.0.1:443: connect: connection refused'),
      'connectionRefused',
    )
    assert.equal(key('context deadline exceeded'), 'timeout')
    assert.equal(
      key(
        'listen tcp 127.0.0.1:7897: bind: Only one usage of each socket address',
      ),
      'portBusy',
    )
  })

  it('узнаёт системные коды Windows, а не только слова', () => {
    // Именно так выглядит блокировка антивирусом или брандмауэром: числом.
    assert.equal(
      key('An attempt was made ... (os error 10013)'),
      'permissionDenied',
    )
  })

  it('частное правило выигрывает у общего', () => {
    // «no such host» содержит и «not found»-подобную суть, но причина разная:
    // домен не разрешился, а не панель ответила 404.
    assert.equal(key('lookup sub.example: no such host'), 'noSuchHost')
    assert.equal(key('unexpected status 404 Not Found'), 'notFound')
  })

  it('незнакомое не переводит', () => {
    // Выдуманный перевод уводит чинить не то — молчим.
    assert.equal(explainErrorKey('boom'), undefined)
    assert.equal(explainErrorKey(''), undefined)
  })
})

describe('trimRawError', () => {
  it('схлопывает переносы и обрезает хвост', () => {
    assert.equal(trimRawError(' a \n  b\t c '), 'a b c')

    const long = 'x'.repeat(RAW_TAIL_LIMIT + 50)
    const trimmed = trimRawError(long)
    assert.equal(trimmed.length, RAW_TAIL_LIMIT)
    assert.ok(trimmed.endsWith('…'))
  })
})
