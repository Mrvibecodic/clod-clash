const PORT =
  /^(?:[1-9]\d{0,3}|[1-5]\d{4}|6[0-4]\d{3}|65[0-4]\d{2}|655[0-2]\d|6553[0-5])$/

const isPortRange = (part: string) => {
  const ends = part.split('-')
  return ends.length <= 2 && ends.every((end) => PORT.test(end.trim()))
}

export const isPortList = (value: string) => value.split('/').every(isPortRange)

/** Ядро не различает регистр сети; в правило она пишется строчными. */
export const normalizeNetwork = (value: string) => value.toLowerCase()

export const isNetwork = (value: string) =>
  ['tcp', 'udp'].includes(normalizeNetwork(value))
