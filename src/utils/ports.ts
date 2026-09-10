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

export const MIN_PORT = 1000
export const MAX_PORT = 65535

export type PortRangeVerdict = 'ok' | 'tooLow' | 'tooHigh'

export const portRangeVerdict = (port: number): PortRangeVerdict => {
  if (port < MIN_PORT) return 'tooLow'
  if (port > MAX_PORT) return 'tooHigh'
  return 'ok'
}

export const findPortOutOfRange = (ports: readonly number[]) => {
  for (const port of ports) {
    if (!port) continue
    const verdict = portRangeVerdict(port)
    if (verdict !== 'ok') return { port, verdict }
  }
  return undefined
}
