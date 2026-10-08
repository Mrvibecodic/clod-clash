/**
 * Удаление фоновой службы целиком делает бэкенд (`uninstall_service`): ядро,
 * работавшее под службой, он останавливает под плановой паузой и после
 * поднимает своим процессом. Окну остаётся сказать, как прошёл каждый шаг, и
 * отказ шага объяснить РОВНО ОДИН раз: раньше один клик давал три одинаковых
 * сообщения.
 */
export interface ServiceUninstallOutcome {
  /** Ядро под службой не остановилось — службу не трогали. */
  stop_error: string | null
  uninstall_error: string | null
  /** Ядро, остановленное ради удаления, поднималось своим процессом. */
  restarted: boolean
  restart_error: string | null
}

export type ServiceUninstallNotice =
  | { done: string }
  | { error: string; consequence?: string }

export const UNINSTALL_STAGE = {
  uninstalling: 'settings.statuses.clashService.uninstalling',
  uninstalled: 'settings.feedback.notifications.clashService.uninstallSuccess',
  skipped: 'settings.feedback.notifications.clashService.uninstallSkipped',
  restarted: 'settings.feedback.notifications.clash.restartSuccess',
} as const

export const serviceUninstallNotices = (
  outcome: ServiceUninstallOutcome,
): ServiceUninstallNotice[] => {
  if (outcome.stop_error !== null) {
    // Человек нажимал «удалить службу», а не «остановить ядро»: одна причина
    // отказа остановки не говорит ему, что служба осталась на месте.
    return [{ error: outcome.stop_error, consequence: UNINSTALL_STAGE.skipped }]
  }
  const notices: ServiceUninstallNotice[] = [
    outcome.uninstall_error === null
      ? { done: UNINSTALL_STAGE.uninstalled }
      : { error: outcome.uninstall_error },
  ]
  if (outcome.restarted) {
    notices.push(
      outcome.restart_error === null
        ? { done: UNINSTALL_STAGE.restarted }
        : { error: outcome.restart_error },
    )
  }
  return notices
}
