import assert from 'node:assert/strict'
import { afterEach, beforeEach, describe, it, mock } from 'node:test'

import { createElement } from 'react'

import {
  collapseBy,
  getSnapshotNotices,
  hideNotice,
  setNoticeWindowVisible,
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

describe('предупреждение', () => {
  beforeEach(clearNotices)

  it('не ошибка и живёт своё время', () => {
    showNotice.warning('shared.feedback.notices.raw', { message: 'нет связи' })

    const notices = getSnapshotNotices()
    assert.equal(notices.length, 1)
    assert.equal(notices[0].type, 'warning')
    assert.equal(notices[0].duration, 6000)
  })

  it('не склеивается с ошибкой того же текста', () => {
    showNotice.warning('shared.feedback.notices.raw', { message: 'нет связи' })
    showNotice.error('shared.feedback.notices.raw', { message: 'нет связи' })

    assert.equal(getSnapshotNotices().length, 2)
  })
})

describe('уведомления в скрытом окне', () => {
  beforeEach(() => {
    clearNotices()
    mock.timers.enable({ apis: ['setTimeout'] })
  })
  afterEach(() => {
    setNoticeWindowVisible(true)
    clearNotices()
    mock.timers.reset()
  })

  it('ждут показа окна и после него живут полный срок', () => {
    setNoticeWindowVisible(false)
    showNotice.error('shared.feedback.notices.raw', { message: 'отказ' })

    mock.timers.tick(60_000)
    assert.equal(getSnapshotNotices().length, 1)

    setNoticeWindowVisible(true)
    mock.timers.tick(7_999)
    assert.equal(getSnapshotNotices().length, 1)
    mock.timers.tick(1)
    assert.equal(getSnapshotNotices().length, 0)
  })

  it('показанное уходит на паузу, когда окно прячут', () => {
    showNotice.error('shared.feedback.notices.raw', { message: 'отказ' })
    mock.timers.tick(4_000)
    setNoticeWindowVisible(false)
    mock.timers.tick(60_000)
    assert.equal(getSnapshotNotices().length, 1)
  })

  it('копится не больше пяти, вечные не вытесняются', () => {
    setNoticeWindowVisible(false)
    showNotice.error('shared.feedback.notices.raw', { message: 'вечное' }, 0)
    for (let index = 0; index < 8; index += 1) {
      showNotice.error('shared.feedback.notices.raw', {
        message: `отказ ${index}`,
      })
    }

    const notices = getSnapshotNotices()
    assert.equal(notices.length, 6)
    assert.equal(notices[0].duration, 0)
    assert.deepEqual(
      notices.slice(1).map((notice) => notice.i18n?.params?.message),
      ['отказ 3', 'отказ 4', 'отказ 5', 'отказ 6', 'отказ 7'],
    )
  })
})
