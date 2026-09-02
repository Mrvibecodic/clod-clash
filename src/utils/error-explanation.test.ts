import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  RAW_TAIL_LIMIT,
  explainErrorKey,
  stripCoreLogPrefix,
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
      key('subscription URL must use https, got scheme "http": http://…'),
      'subscriptionHttpsOnly',
    )
    assert.equal(
      key(
        'listen tcp 127.0.0.1:7897: bind: Only one usage of each socket address',
      ),
      'portBusy',
    )
  })

  it('различает беды защищённого канала', () => {
    // Метка отказа несёт в себе код ответа, а общее правило про 404 стоит
    // ниже — иначе человек чинил бы «адрес не найден» вместо связи.
    assert.equal(key('clod-chan-refused: 404 Not Found'), 'chanRefused')
    assert.equal(key('clod-chan-undecryptable'), 'chanBroken')
    assert.equal(key('clod-chan-stale'), 'chanReplay')
    assert.equal(key('clod-chan-bad-url'), 'chanBadUrl')
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

  it('узнаёт чужой ответ вместо подписки по метке ядра импорта', () => {
    assert.equal(
      key('clod-sub-link-list: the panel returned a base64 link list'),
      'subscriptionLinkList',
    )
    assert.equal(
      key('clod-sub-web-page: the subscription address returned a web page'),
      'subscriptionWebPage',
    )
    assert.equal(
      key(
        'clod-sub-foreign-core: the template relies on a `smart` proxy group',
      ),
      'foreignCoreTemplate',
    )
  })

  it('имя проверочного файла не считается бедой с разбором', () => {
    assert.equal(
      key(
        'Parse config error: /home/u/.local/share/clash-verge-check.yaml: proxy 0: unsupport proxy type: smart',
      ),
      'unsupportedProxy',
    )
    assert.equal(
      key(
        'Parse config error: /home/u/.local/share/clash-verge-check.yaml: proxy Fast not found',
      ),
      'proxyNotFound',
    )
    assert.equal(
      key(
        '/home/u/.local/share/clash-verge-check.yaml: yaml: line 3: mapping values are not allowed',
      ),
      'badConfig',
    )
    assert.equal(
      key('YAML syntax error: did not find expected key'),
      'badConfig',
    )
  })

  it('объясняет собственные жалобы приложения на сборку конфига', () => {
    assert.equal(key('failed to parse config to yaml file'), 'badConfig')
    assert.equal(key('failed to convert config to yaml'), 'badConfig')
    assert.equal(key('YAML generation failed'), 'badConfig')
    assert.equal(
      key('failed to transform to yaml mapping "/home/u/profiles/a.yaml"'),
      'badConfig',
    )
    assert.equal(
      explainErrorKey(
        'failed to save /home/u/.local/share/clash-verge-check.yaml',
      ),
      undefined,
    )
  })

  it('не путается в служебном префиксе лога ядра', () => {
    assert.equal(
      explainErrorKey(
        'time="2026-01-02T03:04:05.502+03:00" level=error msg="boom"',
      ),
      undefined,
    )
    assert.equal(
      explainErrorKey(
        'time="2026-01-02T03:04:05.401+03:00" level=error msg="boom"',
      ),
      undefined,
    )
  })

  it('незнакомое не переводит', () => {
    // Выдуманный перевод уводит чинить не то — молчим.
    assert.equal(explainErrorKey('boom'), undefined)
    assert.equal(explainErrorKey(''), undefined)
  })
})

describe('stripCoreLogPrefix', () => {
  it('снимает обёртку logrus и не трогает всё остальное', () => {
    assert.equal(
      stripCoreLogPrefix(
        'time="2026-01-02T03:04:05+03:00" level=error msg="Parse config error: proxy 0: unsupport proxy type: smart"',
      ),
      'Parse config error: proxy 0: unsupport proxy type: smart',
    )
    assert.equal(stripCoreLogPrefix('plain failure'), 'plain failure')
  })

  it('разэкранирует кавычки и слэши внутри msg', () => {
    assert.equal(
      stripCoreLogPrefix(
        'time="2026-01-02T03:04:05+03:00" level=error msg="he said \\"hi\\""',
      ),
      'he said "hi"',
    )
    assert.equal(
      stripCoreLogPrefix(
        'time="2026-01-02T03:04:05+03:00" level=error msg="open C:\\\\tmp\\\\a: no such file"',
      ),
      'open C:\\tmp\\a: no such file',
    )
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

  it('не тратит длину хвоста на служебный префикс лога', () => {
    const reason = `Parse config error: ${'r'.repeat(RAW_TAIL_LIMIT - 40)}`
    const raw = `time="2026-01-02T03:04:05+03:00" level=error msg="${reason}"`
    assert.equal(trimRawError(raw), reason)
  })
})
