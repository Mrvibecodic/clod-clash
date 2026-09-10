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

type PortRangeVerdict = 'ok' | 'tooLow' | 'tooHigh'

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

const stripBrackets = (host: string) =>
  host.startsWith('[') && host.endsWith(']') ? host.slice(1, -1) : host

export const isProxyServerAt = (
  server: string | undefined | null,
  host: string,
  port: number | undefined | null,
) => {
  if (!server || !port) return false
  const colon = server.lastIndexOf(':')
  if (colon <= 0) return false
  return (
    stripBrackets(server.slice(0, colon)) === stripBrackets(host) &&
    server.slice(colon + 1) === String(port)
  )
}
