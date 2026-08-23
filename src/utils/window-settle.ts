const AFTER_FIRST_FIT_MS = 5000

const BEFORE_FIRST_FIT_MS = 60000

export interface StartupSettle {
  markFitAttempt: (now: number) => void
  markSettled: () => void
  isGrace: (now: number) => boolean
}

export const createStartupSettle = (startedAt: number): StartupSettle => {
  let fitAttemptedAt = 0
  let settled = false

  return {
    markFitAttempt: (now: number) => {
      if (!fitAttemptedAt) fitAttemptedAt = now
    },
    markSettled: () => {
      settled = true
    },
    isGrace: (now: number) =>
      !settled &&
      (fitAttemptedAt
        ? now - fitAttemptedAt < AFTER_FIRST_FIT_MS
        : now - startedAt < BEFORE_FIRST_FIT_MS),
  }
}
