import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  runServiceUninstall,
  type ServiceUninstallSteps,
} from './service-uninstall.ts'

interface Run {
  called: string[]
  said: string[]
  failures: unknown[]
}

const drive = async (steps: Partial<ServiceUninstallSteps>): Promise<Run> => {
  const run: Run = { called: [], said: [], failures: [] }
  const step = (name: keyof ServiceUninstallSteps) => async () => {
    run.called.push(name)
    await steps[name]?.()
  }
  await runServiceUninstall(
    {
      stopCore: step('stopCore'),
      uninstallService: step('uninstallService'),
      restartCore: step('restartCore'),
    },
    {
      busy: (key) => run.said.push(`busy:${key}`),
      done: (key) => run.said.push(`done:${key}`),
      failed: (error) => run.failures.push(error),
    },
  )
  return run
}

const refuses = (why: string) => () => Promise.reject(new Error(why))

describe('удаление фоновой службы', () => {
  it('отказ остановки объясняется один раз, службу не трогает и второе ядро не поднимает', async () => {
    const run = await drive({ stopCore: refuses('не удалось остановить') })
    assert.deepEqual(run.called, ['stopCore'])
    assert.equal(run.failures.length, 1)
    assert.equal((run.failures[0] as Error).message, 'не удалось остановить')
    assert.ok(
      !run.said.some((said) => said.startsWith('done:')),
      'после отказа остановки успеха быть не может',
    )
  })

  it('остановленное ядро поднимается обратно даже когда удаление не удалось', async () => {
    const run = await drive({ uninstallService: refuses('служба занята') })
    assert.deepEqual(run.called, [
      'stopCore',
      'uninstallService',
      'restartCore',
    ])
    assert.equal(run.failures.length, 1)
    assert.ok(
      !run.said.includes(
        'done:settings.feedback.notifications.clashService.uninstallSuccess',
      ),
    )
    assert.ok(
      run.said.includes(
        'done:settings.feedback.notifications.clash.restartSuccess',
      ),
    )
  })

  it('удачный проход докладывает об удалении и о поднятом ядре', async () => {
    const run = await drive({})
    assert.deepEqual(run.called, [
      'stopCore',
      'uninstallService',
      'restartCore',
    ])
    assert.deepEqual(run.failures, [])
    assert.deepEqual(run.said, [
      'busy:settings.statuses.clash.stopping',
      'busy:settings.statuses.clashService.uninstalling',
      'done:settings.feedback.notifications.clashService.uninstallSuccess',
      'busy:settings.statuses.clash.restarting',
      'done:settings.feedback.notifications.clash.restartSuccess',
    ])
  })

  it('провалившийся перезапуск не добавляет второго сообщения об удаче', async () => {
    const run = await drive({ restartCore: refuses('ядро не поднялось') })
    assert.equal(run.failures.length, 1)
    assert.ok(
      !run.said.includes(
        'done:settings.feedback.notifications.clash.restartSuccess',
      ),
    )
  })
})
