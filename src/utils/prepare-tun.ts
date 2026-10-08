/**
 * Довести систему до TUN (`ensure`) и при любом исходе перечитать состояние
 * службы и TUN (`refresh`): служба могла встать или смениться и на шаге,
 * который потом не удался. Сбой перечитывания исход подготовки не меняет.
 */
export const prepareTunWith = async (
  ensure: () => Promise<boolean>,
  refresh: () => Promise<unknown>,
) => {
  try {
    return await ensure()
  } finally {
    await refresh().catch(() => undefined)
  }
}
