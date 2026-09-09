/**
 * Первый порт, который встречается в списке дважды.
 * Отключённые слушатели в список не попадают.
 */
export const findDuplicatePort = (ports: readonly number[]) => {
  const seen = new Set<number>()
  for (const port of ports) {
    if (seen.has(port)) return port
    seen.add(port)
  }
  return undefined
}
