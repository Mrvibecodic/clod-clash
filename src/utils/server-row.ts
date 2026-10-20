import { nameWithoutFlag } from './country.ts'
import { serverName } from './proxy-label.ts'

interface Behind {
  server: string
  via: string
}

export const serverRowNames = (
  current: string,
  behind: Behind | undefined,
  leaf: string | undefined,
  direct: string,
) => ({
  name: serverName(behind?.server ?? current, direct),
  via: behind ? nameWithoutFlag(behind.via) : undefined,
  leaf: leaf === undefined ? undefined : serverName(leaf, direct),
})

export interface DrawerRow {
  selected: boolean
  group: boolean
  builtin: boolean
  typeShown: boolean
  leaf?: string
  behind?: Behind
  typeLabel: string
  nodeType: string
}

export interface DrawerWords {
  direct: string
  inUse: string
  now: (server: string) => string
}

export const drawerRowCaption = (row: DrawerRow, words: DrawerWords) =>
  row.behind
    ? words.now(serverName(row.behind.server, words.direct))
    : row.selected
      ? row.group && row.leaf
        ? `${words.inUse} · ${serverName(row.leaf, words.direct)}`
        : words.inUse
      : row.builtin
        ? ''
        : row.group && row.typeShown
          ? row.leaf
            ? `${row.typeLabel} · ${serverName(row.leaf, words.direct)}`
            : row.typeLabel
          : row.nodeType

export type ClashMode = 'rule' | 'global' | 'direct'

export const clashMode = (mode: string | undefined): ClashMode => {
  const normalized = mode?.toLowerCase()
  return normalized === 'global' || normalized === 'direct'
    ? normalized
    : 'rule'
}

export const trafficGroups = <G>(
  mode: ClashMode,
  visible: G[],
  global: G | undefined,
): G[] =>
  mode === 'direct'
    ? []
    : mode === 'global'
      ? global
        ? [global]
        : []
      : visible
