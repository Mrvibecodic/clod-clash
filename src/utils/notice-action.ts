/**
 * Действие из уведомления: удалось — `done` (уведомление закрывается), не
 * удалось — отказом или исключением — `failed` с причиной.
 */
export const runNoticeAction = (
  act: () => unknown,
  done: () => void,
  failed: (err: unknown) => void,
): Promise<void> => Promise.resolve().then(act).then(done, failed)
