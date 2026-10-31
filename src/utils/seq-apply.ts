const fieldsOf = (value: unknown): Record<string, unknown> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {}

const listOf = (value: unknown): unknown[] =>
  Array.isArray(value) ? value : []

/** Имя записи списка: строка или объект с полем `name`. */
const itemName = (item: unknown): string | undefined => {
  if (typeof item === 'string') return item
  if (item && typeof item === 'object' && 'name' in item) {
    const { name } = item as { name: unknown }
    if (typeof name === 'string') return name
  }
  return undefined
}

/** Имена из `delete`: строками или объектами `{ name }`. */
export const seqDeleteNames = (input?: unknown): string[] =>
  Array.from(
    new Set(
      listOf(input)
        .map(itemName)
        .filter((name): name is string => !!name),
    ),
  )

/** Список подписки после `prepend`/`delete`/`append` из файла правок. */
export const applySeq = <T>(origin: readonly T[], seq: unknown): T[] => {
  const config = fieldsOf(seq)
  const deleted = seqDeleteNames(config.delete)
  return [
    ...(listOf(config.prepend) as T[]),
    ...origin.filter((item) => {
      const name = itemName(item)
      return name ? !deleted.includes(name) : true
    }),
    ...(listOf(config.append) as T[]),
  ]
}

const namesOf = (items: readonly unknown[]) =>
  items
    .map(itemName)
    .filter((name): name is string => typeof name === 'string' && !!name)

/** Что можно поставить в «Прокси» группы: встроенные, группы, затем узлы. */
export const groupPolicyOptions = ({
  builtin,
  prepend,
  origin,
  deleted,
  append,
  proxies,
}: {
  builtin: readonly string[]
  prepend: readonly { name: string }[]
  origin: readonly { name: string }[]
  deleted: readonly string[]
  append: readonly { name: string }[]
  proxies: readonly string[]
}) =>
  Array.from(
    new Set([
      ...builtin,
      ...prepend.map((group) => group.name),
      ...origin
        .map((group) => group.name)
        .filter((name) => !deleted.includes(name)),
      ...append.map((group) => group.name),
      ...proxies,
    ]),
  )

/** Из разобранной подписки и правок — то, что редактор групп берёт с диска. */
export const groupsEditorSource = (
  profile: unknown,
  merge: unknown,
  proxiesSeq: unknown,
) => {
  const origin = fieldsOf(profile)
  const extra = fieldsOf(merge)
  return {
    groups: listOf(origin['proxy-groups']) as IProxyGroupConfig[],
    providers: Object.keys({
      ...fieldsOf(origin['proxy-providers']),
      ...fieldsOf(extra['proxy-providers']),
    }),
    proxyNames: namesOf(applySeq(listOf(origin.proxies), proxiesSeq)),
  }
}
