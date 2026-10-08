import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  type ServiceUninstallOutcome,
  serviceUninstallNotices,
} from './service-uninstall.ts'

const outcome = (
  patch: Partial<ServiceUninstallOutcome>,
): ServiceUninstallOutcome => ({
  stop_error: null,
  uninstall_error: null,
  restarted: false,
  restart_error: null,
  ...patch,
})

describe('удаление фоновой службы', () => {
  it('отказ остановки объясняется один раз и говорит, что служба осталась', () => {
    assert.deepEqual(
      serviceUninstallNotices(outcome({ stop_error: 'не удалось остановить' })),
      [
        {
          error: 'не удалось остановить',
          consequence:
            'settings.feedback.notifications.clashService.uninstallSkipped',
        },
      ],
    )
  })

  it('поднятое обратно ядро докладывается и когда удаление не удалось', () => {
    assert.deepEqual(
      serviceUninstallNotices(
        outcome({ uninstall_error: 'служба занята', restarted: true }),
      ),
      [
        { error: 'служба занята' },
        { done: 'settings.feedback.notifications.clash.restartSuccess' },
      ],
    )
  })

  it('удачный проход докладывает об удалении и о поднятом ядре', () => {
    assert.deepEqual(serviceUninstallNotices(outcome({ restarted: true })), [
      { done: 'settings.feedback.notifications.clashService.uninstallSuccess' },
      { done: 'settings.feedback.notifications.clash.restartSuccess' },
    ])
  })

  it('ядро своим процессом не перезапускалось — о перезапуске ни слова', () => {
    assert.deepEqual(serviceUninstallNotices(outcome({})), [
      { done: 'settings.feedback.notifications.clashService.uninstallSuccess' },
    ])
  })

  it('провалившийся перезапуск не добавляет сообщения об удаче', () => {
    assert.deepEqual(
      serviceUninstallNotices(
        outcome({ restarted: true, restart_error: 'ядро не поднялось' }),
      ),
      [
        {
          done: 'settings.feedback.notifications.clashService.uninstallSuccess',
        },
        { error: 'ядро не поднялось' },
      ],
    )
  })
})
