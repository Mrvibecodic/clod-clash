/**
 * Удаление фоновой службы — три шага. Отказ шага объясняется РОВНО ОДИН раз:
 * раньше один клик во время выхода давал три одинаковых сообщения, потому что
 * ошибку показывал и сам шаг, и перезапуск, и вызывающий поверх них.
 *
 * Отказавшая остановка не значит, что ядро осталось на ходу: режим переводится
 * в «не запущено» до фактического убийства процесса. Поэтому поднять ядро
 * обратно пытаемся в любом случае — оставить его лежащим дороже лишней строки.
 */

export interface ServiceUninstallSteps {
  stopCore: () => Promise<void>
  uninstallService: () => Promise<void>
  restartCore: () => Promise<void>
}

export interface ServiceUninstallTalk {
  busy: (key: string) => void
  done: (key: string) => void
  failed: (error: unknown) => void
}

const UNINSTALL_STAGE = {
  stopping: 'settings.statuses.clash.stopping',
  uninstalling: 'settings.statuses.clashService.uninstalling',
  uninstalled: 'settings.feedback.notifications.clashService.uninstallSuccess',
  restarting: 'settings.statuses.clash.restarting',
  restarted: 'settings.feedback.notifications.clash.restartSuccess',
} as const

export const runServiceUninstall = async (
  steps: ServiceUninstallSteps,
  talk: ServiceUninstallTalk,
): Promise<void> => {
  talk.busy(UNINSTALL_STAGE.stopping)
  try {
    await steps.stopCore()
  } catch (error) {
    talk.failed(error)
    talk.busy(UNINSTALL_STAGE.restarting)
    try {
      await steps.restartCore()
      talk.done(UNINSTALL_STAGE.restarted)
    } catch (restartError) {
      talk.failed(restartError)
    }
    return
  }

  talk.busy(UNINSTALL_STAGE.uninstalling)
  let uninstalled = true
  try {
    await steps.uninstallService()
  } catch (error) {
    uninstalled = false
    talk.failed(error)
  }
  if (uninstalled) {
    talk.done(UNINSTALL_STAGE.uninstalled)
  }

  // Ядро остановили мы — поднять его обратно обязаны независимо от того,
  // удалилась ли служба.
  talk.busy(UNINSTALL_STAGE.restarting)
  try {
    await steps.restartCore()
  } catch (error) {
    talk.failed(error)
    return
  }
  talk.done(UNINSTALL_STAGE.restarted)
}
