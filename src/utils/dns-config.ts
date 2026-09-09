const FORM_DNS_KEYS = [
  'enable',
  'listen',
  'enhanced-mode',
  'fake-ip-range',
  'fake-ip-range6',
  'fake-ip-filter-mode',
  'prefer-h3',
  'respect-rules',
  'use-hosts',
  'use-system-hosts',
  'ipv6',
  'fake-ip-filter',
  'default-nameserver',
  'nameserver',
  'direct-nameserver-follow-policy',
  'proxy-server-nameserver',
  'direct-nameserver',
  'nameserver-policy',
] as const

/** clod:dns-listen — заводской `listen` прежних сборок (03–05.2025). */
const LEGACY_DNS_LISTEN = ':53'

/**
 * Что показать в поле «Слушать»: ровно заводской `:53` прежних сборок — как
 * пустое поле, чтобы следующее сохранение страницы его сняло.
 *
 * clod:Р09-26 — при старте бэкенд снимает `:53` только из файла без
 * комментариев (правленное руками не переписываем), и у остальных страница
 * показывала бы «наш» адрес как выбор человека и клала бы его обратно при
 * каждом сохранении: локальный сокет на 53-м порту конфликтует с системным
 * резолвером. Сам файл до сохранения не трогается. Любое другое значение,
 * в том числе набранное руками `0.0.0.0:53`, показывается как есть.
 */
export function listenFieldFrom(value: unknown): string {
  if (typeof value !== 'string') return ''
  return value.trim() === LEGACY_DNS_LISTEN ? '' : value
}

export function asDnsMapping(
  value: unknown,
): Record<string, unknown> | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    return undefined
  }

  return value as Record<string, unknown>
}

export function readDnsBlock(config: unknown): Record<string, unknown> {
  const root = asDnsMapping(config)
  if (!root) return {}

  if ('dns' in root) return asDnsMapping(root.dns) ?? {}

  const owned = new Set<string>(FORM_DNS_KEYS)
  if (!Object.keys(root).some((key) => owned.has(key))) return {}

  const flat: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(root)) {
    if (key !== 'hosts') flat[key] = value
  }

  return flat
}

export function mergeDnsConfig(
  base: unknown,
  formFields: Record<string, unknown>,
): Record<string, unknown> {
  const source = asDnsMapping(base) ?? {}
  const owned = new Set<string>(FORM_DNS_KEYS)

  const merged: Record<string, unknown> = {}

  for (const [key, value] of Object.entries(source)) {
    if (key in formFields) {
      merged[key] = formFields[key]
    } else if (!owned.has(key)) {
      merged[key] = value
    }
  }

  for (const [key, value] of Object.entries(formFields)) {
    if (!(key in merged)) {
      merged[key] = value
    }
  }

  return merged
}

export function summarizeValidation(outcome: {
  status: string
  message?: string
  reason?: string
}): string {
  const raw =
    outcome.status === 'invalid'
      ? (outcome.message ?? outcome.status)
      : (outcome.reason ?? outcome.status)

  if (!raw.includes('level=error')) return raw

  const lines = raw
    .split('\n')
    .filter(
      (line) =>
        line.includes('level=error') ||
        line.includes('level=fatal') ||
        line.includes('failed'),
    )

  if (lines.length === 0) return raw

  return lines
    .map((line) => {
      const message = line.match(/msg="([^"]+)"/)
      return message ? message[1] : line
    })
    .join(', ')
}
