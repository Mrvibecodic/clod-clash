export const takenSeqNames = (
  ...lists: ReadonlyArray<{ name: string }>[]
): Set<string> => new Set(lists.flat().map((item) => item.name))

export const groupNameConflict = (
  name: string,
  groups: ReadonlyArray<{ name: string }>[],
  proxyNames: readonly string[],
) =>
  takenSeqNames(...groups).has(name)
    ? 'group'
    : proxyNames.includes(name)
      ? 'proxy'
      : null
