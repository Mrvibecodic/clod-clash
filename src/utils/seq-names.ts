import { CORE_POLICIES } from './core-policies.ts'

export const takenSeqNames = (
  ...lists: ReadonlyArray<{ name: string }>[]
): Set<string> => new Set(lists.flat().map((item) => item.name))

export const seqNameConflict = (
  name: string,
  kind: 'group' | 'proxy',
  taken: ReadonlySet<string>,
  proxyNames: readonly string[] = [],
) =>
  CORE_POLICIES.includes(name) || (kind === 'proxy' && name === 'GLOBAL')
    ? 'builtin'
    : taken.has(name)
      ? 'taken'
      : proxyNames.includes(name)
        ? 'proxy'
        : null
