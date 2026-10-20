export interface SeqLists<T> {
  prepend: T[]
  append: T[]
  delete: string[]
}

export const EMPTY_SEQ_LISTS: SeqLists<never> = {
  prepend: [],
  append: [],
  delete: [],
}

/**
 * Есть ли в наглядном виде несохранённое. Если файл на диске не разложился
 * по спискам (открыт текстом), наглядный вид после починки текста — уже
 * отличие от диска.
 */
export const seqUnsaved = <T>(
  ready: boolean,
  baseline: SeqLists<T> | null,
  current: SeqLists<T>,
) =>
  baseline === null
    ? ready
    : JSON.stringify([current.prepend, current.append, current.delete]) !==
      JSON.stringify([baseline.prepend, baseline.append, baseline.delete])
