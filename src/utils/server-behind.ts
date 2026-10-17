interface Choice {
  now?: string
  all?: unknown
}

/**
 * Куда ведёт выбор от `name`: запись в конце пути и группа, в которой она
 * выбрана (`undefined` — `name` сам ничего не выбирает). Скрытые группы — тоже
 * на пути: ядро отдаёт их в общем списке.
 */
export const selectionEnd = (
  records: Record<string, Choice | undefined>,
  name: string,
) => {
  let leaf = name
  let via: string | undefined
  for (let hop = 0; hop < 6; hop += 1) {
    const record = records[leaf]
    if (!record?.now || record.now === leaf) break
    via = leaf
    leaf = record.now
  }
  return { leaf, via }
}

/**
 * Сервер, на котором стоит группа `name`, и группа, в которой он выбран.
 * Путь кончается группой (балансировка, цикл) или заглушкой ядра (`skip`) —
 * сервера нет.
 */
export const serverBehind = (
  records: Record<string, Choice | undefined>,
  name: string,
  skip: (name: string) => boolean,
) => {
  const { leaf, via } = selectionEnd(records, name)
  if (via === undefined || records[leaf]?.all || skip(leaf)) return undefined
  return { server: leaf, via }
}
