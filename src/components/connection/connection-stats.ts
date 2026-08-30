// Подсчёты для вкладки «Соединения»: группировка строк таблицы и сводка над
// ней. Вынесены из компонентов отдельным модулем, чтобы их можно было
// проверить тестами: React и i18n сюда не импортируются намеренно.
const TOP_ITEMS = 4
const DIRECT_OUTBOUND = 'DIRECT'

export interface ConnectionGroup {
  key: string
  label: string
  rows: IConnectionsItem[]
  download: number
  upload: number
  downloadSpeed: number
  uploadSpeed: number
}

export interface ConnectionSummaryEntry {
  key: string
  label: string
  value: number
}

export interface ConnectionSummaryStats {
  download: number
  upload: number
  downloadSpeed: number
  uploadSpeed: number
  processCount: number
  processes: ConnectionSummaryEntry[]
  routes: ConnectionSummaryEntry[]
}

export const shortProcessName = (value: string) => {
  const separator = Math.max(value.lastIndexOf('/'), value.lastIndexOf('\\'))
  return separator >= 0 ? value.slice(separator + 1) : value
}

export const buildConnectionGroups = (
  connections: readonly IConnectionsItem[],
  resolveKey: (connection: IConnectionsItem) => string,
  fallbackLabel: string,
): ConnectionGroup[] => {
  const groups = new Map<string, ConnectionGroup>()

  for (let i = 0; i < connections.length; i++) {
    const connection = connections[i]
    const key = resolveKey(connection) || fallbackLabel

    let group = groups.get(key)
    if (!group) {
      group = {
        key,
        label: key,
        rows: [],
        download: 0,
        upload: 0,
        downloadSpeed: 0,
        uploadSpeed: 0,
      }
      groups.set(key, group)
    }

    group.rows.push(connection)
    group.download += connection.download ?? 0
    group.upload += connection.upload ?? 0
    group.downloadSpeed += connection.curDownload ?? 0
    group.uploadSpeed += connection.curUpload ?? 0
  }

  // Сверху тот, кто съел больше всех: ради этого группировку и включают.
  return [...groups.values()].sort(
    (left, right) =>
      right.download + right.upload - (left.download + left.upload),
  )
}

const topEntries = (totals: Map<string, number>): ConnectionSummaryEntry[] =>
  [...totals.entries()]
    .sort((left, right) => right[1] - left[1])
    .slice(0, TOP_ITEMS)
    .map(([key, value]) => ({ key, label: key, value }))

export const summarizeConnections = (
  connections: readonly IConnectionsItem[],
  labels: { noProcess: string; direct: string },
): ConnectionSummaryStats => {
  let download = 0
  let upload = 0
  let downloadSpeed = 0
  let uploadSpeed = 0
  const processTotals = new Map<string, number>()
  const routeTotals = new Map<string, number>()

  for (let i = 0; i < connections.length; i++) {
    const connection = connections[i]
    const rowDownload = connection.download ?? 0
    const rowUpload = connection.upload ?? 0
    const rowTotal = rowDownload + rowUpload

    download += rowDownload
    upload += rowUpload
    downloadSpeed += connection.curDownload ?? 0
    uploadSpeed += connection.curUpload ?? 0

    const process =
      connection.metadata.process || connection.metadata.processPath || ''
    const processKey = process ? shortProcessName(process) : labels.noProcess
    processTotals.set(
      processKey,
      (processTotals.get(processKey) ?? 0) + rowTotal,
    )

    // chains[0] — реальный исход соединения: ядро дописывает цепочку изнутри
    // наружу, поэтому первый элемент и есть узел, через который ушёл трафик.
    const outbound = connection.chains[0] ?? ''
    const routeKey =
      outbound === DIRECT_OUTBOUND
        ? labels.direct
        : outbound || labels.noProcess
    routeTotals.set(routeKey, (routeTotals.get(routeKey) ?? 0) + rowTotal)
  }

  return {
    download,
    upload,
    downloadSpeed,
    uploadSpeed,
    processCount: processTotals.size,
    processes: topEntries(processTotals),
    routes: topEntries(routeTotals),
  }
}
