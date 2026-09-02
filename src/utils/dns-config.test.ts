import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  mergeDnsConfig,
  readDnsBlock,
  summarizeValidation,
} from './dns-config.ts'

describe('readDnsBlock', () => {
  it('обёрнутый блок берётся из ключа dns', () => {
    assert.deepEqual(readDnsBlock({ dns: { enable: true }, hosts: {} }), {
      enable: true,
    })
  })

  it('ключ dns с невнятным значением даёт пустой блок', () => {
    assert.deepEqual(readDnsBlock({ dns: null }), {})
    assert.deepEqual(readDnsBlock({ dns: 'nope' }), {})
    assert.deepEqual(readDnsBlock({ dns: ['a'] }), {})
  })

  it('плоский старый файл читается как блок dns', () => {
    assert.deepEqual(
      readDnsBlock({
        enable: true,
        ipv6: false,
        nameserver: ['1.1.1.1'],
        'cache-algorithm': 'arc',
      }),
      {
        enable: true,
        ipv6: false,
        nameserver: ['1.1.1.1'],
        'cache-algorithm': 'arc',
      },
    )
  })

  it('hosts из плоского файла в блок dns не затягивается', () => {
    assert.deepEqual(readDnsBlock({ enable: true, hosts: { 'a.test': '1' } }), {
      enable: true,
    })
  })

  it('документ без узнаваемых ключей резолвера блоком dns не считается', () => {
    assert.deepEqual(readDnsBlock({ hosts: { 'a.test': '1' } }), {})
    assert.deepEqual(readDnsBlock({}), {})
    assert.deepEqual(readDnsBlock(undefined), {})
    assert.deepEqual(readDnsBlock('hello'), {})
    assert.deepEqual(readDnsBlock(['a']), {})
  })
})

describe('mergeDnsConfig', () => {
  it('незнакомые ключи разобранного блока переживают сохранение формы', () => {
    const merged = mergeDnsConfig(
      {
        'cache-algorithm': 'arc',
        'cache-max-size': 4096,
        'fake-ip-ttl': 300,
        'ipv6-timeout': 200,
        nameserver: ['1.1.1.1'],
      },
      { nameserver: ['8.8.8.8'], enable: true },
    )

    assert.deepEqual(merged, {
      'cache-algorithm': 'arc',
      'cache-max-size': 4096,
      'fake-ip-ttl': 300,
      'ipv6-timeout': 200,
      nameserver: ['8.8.8.8'],
      enable: true,
    })
  })

  it('поля формы перекрывают исходные и сохраняют порядок блока', () => {
    const merged = mergeDnsConfig(
      { enable: false, 'cache-algorithm': 'lru', ipv6: false },
      { enable: true, ipv6: true },
    )

    assert.deepEqual(Object.keys(merged), ['enable', 'cache-algorithm', 'ipv6'])
    assert.equal(merged.enable, true)
    assert.equal(merged.ipv6, true)
  })

  it('поле формы, которого форма не отдала, из блока убирается', () => {
    const merged = mergeDnsConfig(
      { 'nameserver-policy': { 'a.test': '1.1.1.1' }, 'cache-max-size': 10 },
      { enable: true },
    )

    assert.deepEqual(merged, { 'cache-max-size': 10, enable: true })
  })

  it('чужой fallback остаётся: заводским его не делаем, но и не выбрасываем', () => {
    const merged = mergeDnsConfig(
      { fallback: ['1.0.0.1'], 'fallback-filter': { geoip: true } },
      { enable: true },
    )

    assert.deepEqual(merged.fallback, ['1.0.0.1'])
    assert.deepEqual(merged['fallback-filter'], { geoip: true })
  })

  it('пустой исходный блок оставляет только поля формы', () => {
    assert.deepEqual(mergeDnsConfig(undefined, { enable: true }), {
      enable: true,
    })
  })

  it('база-строка и база-список считаются пустой базой', () => {
    assert.deepEqual(mergeDnsConfig('hello', { enable: true }), {
      enable: true,
    })
    assert.deepEqual(mergeDnsConfig(['a', 'b'], { enable: true }), {
      enable: true,
    })
    assert.deepEqual(mergeDnsConfig(42, { enable: true }), { enable: true })
  })

  it('поле формы со значением умолчания всё равно попадает в блок', () => {
    const merged = mergeDnsConfig(
      { nameserver: ['1.1.1.1'] },
      {
        enable: true,
        nameserver: ['1.1.1.1'],
        listen: ':53',
        'use-hosts': false,
        'use-system-hosts': false,
        'respect-rules': false,
        'fake-ip-range6': '2001:2::0/64',
        'fake-ip-filter-mode': 'blacklist',
        'direct-nameserver-follow-policy': false,
      },
    )

    assert.deepEqual(merged, {
      nameserver: ['1.1.1.1'],
      enable: true,
      listen: ':53',
      'use-hosts': false,
      'use-system-hosts': false,
      'respect-rules': false,
      'fake-ip-range6': '2001:2::0/64',
      'fake-ip-filter-mode': 'blacklist',
      'direct-nameserver-follow-policy': false,
    })
  })

  it('маленький засеянный блок сохраняется полным набором полей', () => {
    const formFields = {
      enable: true,
      listen: ':53',
      ipv6: true,
      nameserver: ['8.8.8.8'],
      'enhanced-mode': 'fake-ip',
    }
    const merged = mergeDnsConfig({ nameserver: ['8.8.8.8'] }, formFields)

    assert.deepEqual(Object.keys(merged).sort(), Object.keys(formFields).sort())
    assert.equal(merged.enable, true)
  })

  it('пустой набор полей на пустой базе даёт пустой блок', () => {
    assert.deepEqual(mergeDnsConfig({}, {}), {})
  })

  it('нетронутое сложное значение переносится из базы без разбора', () => {
    const policy = { 'a.test': ['1.1.1.1', '8.8.8.8'], 'b.test': null }
    const merged = mergeDnsConfig(
      { 'nameserver-policy': policy },
      { 'nameserver-policy': policy },
    )

    assert.deepEqual(merged['nameserver-policy'], policy)
  })
})

describe('summarizeValidation', () => {
  it('вердикт ядра показывается его же сообщением', () => {
    assert.equal(
      summarizeValidation({
        status: 'invalid',
        message: 'not found rule-set: ru',
      }),
      'not found rule-set: ru',
    )
  })

  it('из журнала ядра остаются только строки об ошибке', () => {
    const raw = [
      'time="1" level=info msg="loading"',
      'time="2" level=error msg="parse config error"',
      'time="3" level=fatal msg="initial configure failed"',
    ].join('\n')

    assert.equal(
      summarizeValidation({ status: 'invalid', message: raw }),
      'parse config error, initial configure failed',
    )
  })

  it('строка без msg переносится как есть', () => {
    const raw = 'time="1" level=info msg="loading"\nlevel=error something broke'
    assert.equal(
      summarizeValidation({ status: 'invalid', message: raw }),
      'level=error something broke',
    )
  })

  it('невыполненная проверка называет свою причину', () => {
    assert.equal(
      summarizeValidation({ status: 'skipped', reason: 'exiting' }),
      'exiting',
    )
    assert.equal(summarizeValidation({ status: 'busy' }), 'busy')
  })
})
