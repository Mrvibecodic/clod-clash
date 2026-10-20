import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

import { tunSetupKey } from './tun-notice.ts'

const read = (relative: string) =>
  readFileSync(new URL(relative, import.meta.url), 'utf8')

const refusalStatus = (): string => {
  const found = /EXIT_REFUSAL_STATUS: &str = "([^"]+)"/.exec(
    read('../../src-tauri/src/core/notification.rs'),
  )
  assert.ok(found, 'в notification.rs нет общего статуса отказа')
  return found[1]
}

describe('подготовка службы для TUN', () => {
  it('отказ «идёт выход» объясняется, а не читается как отсутствие службы', () => {
    assert.equal(
      tunSetupKey(refusalStatus()),
      'settings.sections.system.notifications.core.exitInProgress',
    )
  })

  it('подготовка отвечает отказом, а не тем же значением, что «службы нет»', () => {
    const source = read('../../src-tauri/src/feat/tun.rs')
    assert.match(
      source,
      /Self::Exiting => SetupAnswer::Refused\(EXIT_REFUSAL_STATUS\)/,
      'исход «идёт выход» снова неотличим от отсутствующей службы',
    )
  })

  it('копия на томе без прав доступа получает объяснение, а не предложение поставить службу', () => {
    assert.equal(
      tunSetupKey('tun::volume_without_rights'),
      'settings.sections.system.notifications.tunMode.volumeWithoutRights',
    )
    assert.match(
      read('../../src-tauri/src/feat/tun.rs'),
      /Self::Unserviceable => SetupAnswer::Refused\("tun::volume_without_rights"\)/,
    )
  })

  it('незнакомая ошибка остаётся как есть', () => {
    assert.equal(tunSetupKey('tun::start_failed'), undefined)
    assert.equal(tunSetupKey(undefined), undefined)
  })
})
