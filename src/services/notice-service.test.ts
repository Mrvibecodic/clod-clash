import assert from 'node:assert/strict'
import { beforeEach, describe, it } from 'node:test'

import { createElement } from 'react'

import {
  collapseBy,
  getSnapshotNotices,
  hideNotice,
  showNotice,
} from './notice-service.ts'

const clearNotices = () => {
  for (const notice of [...getSnapshotNotices()]) {
    hideNotice(notice.id)
  }
}

const portBusy = (port: string) =>
  showNotice.error(
    createElement('span', null, `Порт ${port} занят`),
    collapseBy(`core::port_busy|${port}`),
    0,
  )

describe('схлопывание уведомлений, собранных как ReactNode', () => {
  beforeEach(clearNotices)

  it('пять одинаковых отказов дают один вечный тост со счётчиком', () => {
    const ids = [portBusy('7890'), portBusy('7890'), portBusy('7890')]

    const notices = getSnapshotNotices()
    assert.equal(notices.length, 1)
    assert.equal(notices[0].repeats, 3)
    assert.equal(new Set(ids).size, 1, 'вызовы должны вернуть один и тот же id')
  })

  it('разные порты остаются разными уведомлениями', () => {
    portBusy('7890')
    portBusy('7891')

    assert.equal(getSnapshotNotices().length, 2)
  })

  it('разные статусы с одинаковым текстом не склеиваются', () => {
    const element = createElement('span', null, 'Системный прокси')
    showNotice.error(element, collapseBy('sysproxy::core_gave_up|'), 0)
    showNotice.error(element, collapseBy('sysproxy::core_not_running|'), 0)

    assert.equal(getSnapshotNotices().length, 2)
  })

  it('ключ схлопывания не съедает ни текст, ни длительность', () => {
    portBusy('7890')

    const [notice] = getSnapshotNotices()
    assert.equal(notice.duration, 0, 'тост с кнопкой должен остаться вечным')
    assert.equal(notice.i18n, undefined)
    assert.ok(notice.message, 'текст уведомления не должен потеряться')
  })

  it('уведомления с переводом по-прежнему схлопываются по описанию', () => {
    showNotice.error('settings.sections.system.notifications.core.crashed', {
      code: 1,
    })
    showNotice.error('settings.sections.system.notifications.core.crashed', {
      code: 1,
    })

    const notices = getSnapshotNotices()
    assert.equal(notices.length, 1)
    assert.equal(notices[0].repeats, 2)
  })
})
