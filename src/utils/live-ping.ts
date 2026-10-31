/** Замер текущего узла моложе этого Главная сама не повторяет. */
const PING_MAX_AGE_MS = 60_000
export const PING_GAP_MS = 60_000
export const PING_RETRY_MS = 5_000

export const groupTestKey = (group: string, updatedAt: number, nodes: string) =>
  `${group}|${updatedAt}|${nodes}`

/** Автозамер группы ещё впереди. `''` — автозамера не будет. */
export const groupTestAhead = (key: string, doneKey: string) =>
  key !== '' && key !== doneKey

interface LivePingState {
  now: number
  measuredAt: number
  lastPingAt: number
  hasPing: boolean
  /** Текущий узел померит пакет: автозамер впереди или узел уже в очереди. */
  batchOwns: boolean
}

export const livePingDue = ({
  now,
  measuredAt,
  lastPingAt,
  hasPing,
  batchOwns,
}: LivePingState) => {
  if (measuredAt && now - measuredAt < PING_MAX_AGE_MS) return false
  if (now - lastPingAt < (hasPing ? PING_GAP_MS : PING_RETRY_MS)) return false
  return !batchOwns
}

/**
 * Замер текущего узла, снятый автозамером в первые мгновения, засчитывается
 * пакету: он идёт или дошёл до ядра. Отказ вызова (-1) — не замер, узел
 * перемеряет пакет. Тайм-аут ядра (0) — замер.
 */
export const firstMeasureCounts = (delay: number | undefined) =>
  delay !== undefined && delay !== -1

/** Узлы автозамера без того, что уже померено по тем же узлам ядра. */
export const nodesLeftToTest = <T extends { name: string }>(
  nodes: readonly T[],
  measured: string | undefined,
) =>
  measured === undefined
    ? [...nodes]
    : nodes.filter((node) => node.name !== measured)
