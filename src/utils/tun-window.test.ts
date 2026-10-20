import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  TUN_RESET,
  dnsHijackSetting,
  mtuIsValid,
  tunFieldsFrom,
  tunOverrides,
  tunPatch,
  tunSavePatch,
  tunUnsavedCount,
} from './tun-window.ts'

// Так выглядит tun в clash.yaml свежей установки.
const TEMPLATE = {
  enable: false,
  stack: 'gvisor',
  'auto-route': true,
  'strict-route': false,
  'auto-detect-interface': true,
  'dns-hijack': ['any:53'],
}
const RUNTIME = { ...TEMPLATE, mtu: 1280, device: 'from-subscription' }

describe('окно TUN', () => {
  it('открыли и сохранили без изменений — в файл ничего не пишется', () => {
    for (const os of ['windows', 'linux', 'macos']) {
      const initial = tunFieldsFrom(TEMPLATE, RUNTIME, os)
      assert.deepEqual(tunPatch(initial, { ...initial }, os), {})
    }
  })

  it('незакреплённое поле пусто, а не выдуманное умолчание', () => {
    const fields = tunFieldsFrom(TEMPLATE, RUNTIME, 'windows')
    assert.equal(fields.mtu, '')
    assert.equal(fields.device, '')
    assert.equal(fields.routeExcludeAddress, '')
  })

  it('закреплённое показывается и не трогается, пока его не меняли', () => {
    const pinned = {
      ...TEMPLATE,
      mtu: 1400,
      'route-exclude-address': ['10.0.0.0/8'],
    }
    const initial = tunFieldsFrom(pinned, RUNTIME, 'linux')
    assert.equal(initial.mtu, '1400')
    assert.equal(initial.routeExcludeAddress, '10.0.0.0/8')
    assert.deepEqual(
      tunPatch(initial, { ...initial, autoDetectInterface: false }, 'linux'),
      { 'auto-detect-interface': false },
    )
  })

  it('очищенное поле снимает закрепление, введённое — закрепляет', () => {
    const initial = tunFieldsFrom(
      { ...TEMPLATE, mtu: 1400 },
      RUNTIME,
      'windows',
    )
    assert.deepEqual(tunPatch(initial, { ...initial, mtu: '' }, 'windows'), {
      mtu: null,
    })
    const empty = tunFieldsFrom(TEMPLATE, RUNTIME, 'windows')
    assert.deepEqual(tunPatch(empty, { ...empty, mtu: '1400' }, 'windows'), {
      mtu: 1400,
    })
  })

  it('перехват DNS без единой записи — «как в подписке»', () => {
    for (const empty of [undefined, '', ',', ' , ,', ',,,']) {
      assert.equal(dnsHijackSetting(empty), 'auto', JSON.stringify(empty))
    }
    assert.equal(dnsHijackSetting('auto'), 'auto')
    assert.equal(dnsHijackSetting('any:53, '), 'any:53, ')
  })

  it('MTU — пусто или целое больше нуля', () => {
    assert.ok(mtuIsValid(''))
    assert.ok(mtuIsValid('9000'))
    assert.ok(!mtuIsValid('0'))
    assert.ok(!mtuIsValid('-1'))
    assert.ok(!mtuIsValid('1.5'))
  })

  it('«Сбросить» снимает ключи окна и не закрепляет 1500 и имя адаптера', () => {
    assert.deepEqual(TUN_RESET, {
      device: null,
      mtu: null,
      'route-exclude-address': null,
      'auto-redirect': null,
      'auto-route': true,
      'auto-detect-interface': true,
    })
    const fresh = tunFieldsFrom(TUN_RESET, undefined, 'linux')
    assert.equal(fresh.mtu, '')
    assert.equal(fresh.device, '')
    assert.equal(fresh.autoRoute, true)
  })

  it('«Сбросить настройки» только заполняет форму: пишется по «Сохранить», правки после сброса — поверх', () => {
    const pinned = { ...TEMPLATE, mtu: 1400, 'auto-redirect': true }
    const initial = tunFieldsFrom(pinned, RUNTIME, 'linux')
    const fresh = tunFieldsFrom(TUN_RESET, undefined, 'linux')
    assert.deepEqual(tunSavePatch(initial, fresh, fresh, 'linux'), TUN_RESET)
    assert.deepEqual(
      tunSavePatch(initial, { ...fresh, mtu: '1380' }, fresh, 'linux'),
      { ...TUN_RESET, mtu: 1380 },
    )
    assert.deepEqual(
      tunSavePatch(initial, { ...initial, mtu: '1380' }, null, 'linux'),
      { mtu: 1380 },
    )
  })

  it('после «Сбросить настройки» несохранённое считается от файла: снятое закрепление — правка', () => {
    const defaults = { stack: 'auto', strictRoute: 'auto', dnsHijack: 'auto' }
    const fresh = tunFieldsFrom(TUN_RESET, undefined, 'linux')
    const pinned = { ...TEMPLATE, 'auto-redirect': false }
    const initial = tunFieldsFrom(pinned, RUNTIME, 'linux')
    const reset = tunSavePatch(initial, fresh, fresh, 'linux')
    assert.equal(
      tunUnsavedCount(
        pinned,
        reset,
        tunOverrides(defaults),
        tunOverrides(defaults),
      ),
      1,
    )
    assert.equal(
      tunUnsavedCount(
        TEMPLATE,
        reset,
        tunOverrides(defaults),
        tunOverrides(defaults),
      ),
      0,
    )
  })

  it('несохранённое: переопределения считаются от сохранённых, неверный MTU — правка', () => {
    const defaults = { stack: 'auto', strictRoute: 'auto', dnsHijack: 'auto' }
    const initial = tunFieldsFrom(TEMPLATE, RUNTIME, 'windows')
    assert.equal(
      tunUnsavedCount(
        TEMPLATE,
        tunPatch(initial, { ...initial, mtu: 'abc' }, 'windows'),
        tunOverrides(defaults),
        tunOverrides({ ...defaults, stack: 'gvisor' }),
      ),
      2,
    )
    assert.equal(
      tunUnsavedCount(
        TEMPLATE,
        tunPatch(initial, { ...initial }, 'windows'),
        tunOverrides(defaults),
        tunOverrides({ ...defaults, dnsHijack: ' , ' }),
      ),
      0,
    )
  })
})
