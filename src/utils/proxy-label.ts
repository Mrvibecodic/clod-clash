/**
 * Подпись узла из подписки — если она про тот же протокол, что у ядра. Иначе
 * (узел ядро завело не из этой записи) подписи нет: лучше никакой, чем чужая.
 */
export const labelFor = (type: string, label: IProxyLabel | undefined) => {
  const core = type.toLowerCase().replace(/^socks5$/, 'socks')
  return typeof label?.proto === 'string' && label.proto.toLowerCase() === core
    ? label
    : undefined
}

/**
 * Значение по имени — только своё: узел может зваться `constructor` или
 * `__proto__`, и обычный доступ отдал бы то, что лежит у прототипа.
 */
export const own = <T>(map: Record<string, T> | undefined, key: string) =>
  map && Object.hasOwn(map, key) ? map[key] : undefined

/**
 * Имена, у которых в разных источниках (сама подписка, провайдеры) разные
 * подписи или где-то подписи нет: какой из одноимённых узлов перед глазами,
 * не угадать — подписи у них не будет.
 */
export const ambiguousNames = (
  entries: Iterable<readonly [string, IProxyLabel | undefined]>,
) => {
  const first = new Map<string, string>()
  const out = new Set<string>()
  for (const [name, label] of entries) {
    const text = label?.text ?? ''
    const seen = first.get(name)
    if (seen === undefined) first.set(name, text)
    else if (seen !== text) out.add(name)
  }
  return out
}

/**
 * Прятать ли тип узла: провайдер спрятал плашки (`clod-hide-badges`), а это
 * не группа. Тип группы (Selector, URLTest) — не протокол и остаётся; у
 * встроенных DIRECT и REJECT состава нет — их тип прячется, как у серверов.
 */
export const hidesType = (
  proxy: { all?: unknown } | undefined,
  hidden: boolean | undefined,
) => !!hidden && !proxy?.all

/**
 * Плашки узла: протокол, транспорт и защита из подписки — сколько их есть.
 * Подписи нет — одна плашка с типом от ядра. Провайдер спрятал плашки
 * (`hidden`, заголовок `clod-hide-badges`) — ни одной, кроме типа группы.
 */
export const typeChips = (
  proxy: Pick<IProxyItem, 'type' | 'label' | 'all'>,
  hidden = false,
) =>
  hidesType(proxy, hidden)
    ? []
    : proxy.label
      ? [proxy.label.proto, proxy.label.transport, proxy.label.security].filter(
          (part): part is string => !!part,
        )
      : [proxy.type]

/** Пометки TFO, MPTCP и SMUX — тоже про транспорт: прячутся вместе с плашками. */
export const featureChips = (
  proxy: Pick<IProxyItem, 'tfo' | 'mptcp' | 'smux'>,
  hidden = false,
) =>
  hidden
    ? []
    : (['tfo', 'mptcp', 'smux'] as const)
        .filter((feature) => proxy[feature])
        .map((feature) => feature.toUpperCase())

/** То же одной строкой — подпись под именем узла; спрятано — пусто. */
export const typeText = (
  node: { type?: string; label?: IProxyLabel; all?: unknown },
  hidden = false,
) => (hidesType(node, hidden) ? '' : (node.label?.text ?? node.type ?? ''))
