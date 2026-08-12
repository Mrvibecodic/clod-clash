/**
 * clod:error-mapper — сырая ошибка ядра → человеческая фраза.
 *
 * Ядро и системные вызовы говорят на своём языке: «context deadline exceeded»,
 * «os error 10013», «dial tcp: lookup … no such host». Пользователь видел это
 * как есть и не мог понять ни что случилось, ни что делать. Словарь ниже
 * переводит распознанное в одну фразу; исходный текст при этом не выбрасывается
 * (он нужен и для поддержки, и для тех, кто читать умеет).
 *
 * Правила разбора:
 * * порядок ВАЖЕН — первое совпадение выигрывает, поэтому частные образцы стоят
 *   выше общих («no such host» до «lookup»);
 * * не узнали — не выдумываем: возвращается `undefined`, и текст показывается
 *   как раньше. Ошибочный перевод хуже непонятного оригинала: он уводит
 *   человека чинить не то;
 * * образцы ищутся в тексте, приведённом к нижнему регистру, без учёта места:
 *   ядро оборачивает свои ошибки в чужие обёртки на каждом слое.
 */
const RULES: ReadonlyArray<{ pattern: RegExp; key: string }> = [
  // --- Защищённый канал ----------------------------------------------------
  // clod:chan — стоят ПЕРВЫМИ: метка отказа содержит код ответа («404»),
  // и общее правило про 404 перехватило бы её, объяснив совсем не то.
  {
    pattern: /clod-chan-(undecryptable|version|bad-key|seal|kdf)/,
    key: 'chanBroken',
  },
  { pattern: /clod-chan-(mismatch|stale)/, key: 'chanReplay' },
  { pattern: /clod-chan-refused/, key: 'chanRefused' },
  { pattern: /clod-chan-bad-url/, key: 'chanBadUrl' },

  // --- Сеть до сервера -----------------------------------------------------
  {
    pattern: /no such host|dns lookup failed|lookup .*: no/,
    key: 'noSuchHost',
  },
  { pattern: /connection refused|actively refused/, key: 'connectionRefused' },
  {
    pattern: /connection reset|reset by peer|forcibly closed/,
    key: 'connectionReset',
  },
  {
    pattern: /context deadline exceeded|i\/o timeout|timed? ?out|timeout/,
    key: 'timeout',
  },
  { pattern: /network is unreachable|no route to host/, key: 'unreachable' },

  // --- Права и порты -------------------------------------------------------
  {
    pattern:
      /address already in use|only one usage of each socket address|os error 10048|os error 98/,
    key: 'portBusy',
  },
  {
    pattern:
      /access permissions|os error 10013|os error 13|permission denied|access is denied|os error 5/,
    key: 'permissionDenied',
  },

  // --- TLS и подпись -------------------------------------------------------
  {
    pattern: /certificate|x509|tls: |handshake failure/,
    key: 'tls',
  },

  // --- Конфигурация --------------------------------------------------------
  {
    pattern: /yaml|cannot unmarshal|unmarshal errors|invalid config/,
    key: 'badConfig',
  },
  {
    pattern: /unsupported proxy type|unsupport proxy type|unsupported type/,
    key: 'unsupportedProxy',
  },
  { pattern: /proxy .* not found|proxy not found/, key: 'proxyNotFound' },

  // --- Туннель -------------------------------------------------------------
  {
    pattern:
      /configure tun interface|start tun listening|create tun|tun device/,
    key: 'tunFailed',
  },

  // --- Ответ панели --------------------------------------------------------
  { pattern: /\b401\b|unauthorized/, key: 'unauthorized' },
  { pattern: /\b403\b|forbidden/, key: 'forbidden' },
  { pattern: /\b404\b|not found/, key: 'notFound' },
  {
    pattern: /\b5\d\d\b|internal server error|bad gateway/,
    key: 'serverError',
  },
]

/** Максимум исходного текста рядом с объяснением. */
export const RAW_TAIL_LIMIT = 160

/**
 * Ключ объяснения для сырого текста ошибки, если он узнан.
 *
 * Возвращается ПОЛНЫЙ ключ i18n, чтобы вызывающему не приходилось помнить
 * префикс — словарь и его расположение остаются одной деталью реализации.
 */
export const explainErrorKey = (raw: string): string | undefined => {
  const text = raw.toLowerCase()
  const rule = RULES.find(({ pattern }) => pattern.test(text))
  return rule ? `shared.feedback.errors.core.${rule.key}` : undefined
}

/** Обрезать исходный текст до хвоста, который ещё уместно показать рядом. */
export const trimRawError = (raw: string): string => {
  const compact = raw.replace(/\s+/g, ' ').trim()
  return compact.length > RAW_TAIL_LIMIT
    ? `${compact.slice(0, RAW_TAIL_LIMIT - 1)}…`
    : compact
}
