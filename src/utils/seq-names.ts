/**
 * Имена, уже занятые в редакторе: добавленные в начало, исходные (и
 * помеченные к удалению — их строки остаются в списке) и добавленные в конец.
 */
export const takenSeqNames = (
  ...lists: ReadonlyArray<{ name: string }>[]
): Set<string> => new Set(lists.flat().map((item) => item.name))
