import { useMemo, useRef } from 'react'

import parseTraffic from '@/utils/parse-traffic'

export interface ConnectionRowView {
  id: string
  host: string
  process: string
  network: string
  type: string
  chains: string
  time: string
  uploadSpeedText: string
  downloadSpeedText: string
  uploadSpeed: number
  downloadSpeed: number
}

export const formatConnectionTraffic = (value?: number) => {
  const [text, unit] = parseTraffic(value)
  return unit ? `${text} ${unit}` : text
}

export const formatConnectionChains = (chains: string[]) => {
  let value = ''
  for (let i = chains.length - 1; i >= 0; i -= 1) {
    if (value) value += ' / '
    value += chains[i]
  }
  return value
}

export const getConnectionDestination = (connection: IConnectionsItem) => {
  const { metadata } = connection
  return metadata.destinationIP
    ? `${metadata.destinationIP}:${metadata.destinationPort}`
    : `${metadata.remoteDestination}:${metadata.destinationPort}`
}

export const getConnectionHost = (connection: IConnectionsItem) => {
  const { metadata } = connection
  const host =
    metadata.host || metadata.destinationIP || metadata.remoteDestination
  return `${host}:${metadata.destinationPort}`
}

export const getConnectionProcess = (connection: IConnectionsItem) => {
  const { metadata } = connection
  return metadata.process || metadata.processPath || ''
}

export const getConnectionRule = (connection: IConnectionsItem) => {
  const { rulePayload } = connection
  return rulePayload ? `${connection.rule}(${rulePayload})` : connection.rule
}

export const getConnectionSource = (connection: IConnectionsItem) => {
  const { metadata } = connection
  return `${metadata.sourceIP}:${metadata.sourcePort}`
}

export const getConnectionTypeLabel = (connection: IConnectionsItem) => {
  const { metadata } = connection
  return `${metadata.type}(${metadata.network})`
}

export const getConnectionStartTime = (connection: IConnectionsItem) =>
  Date.parse(connection.start || '') || 0

const createConnectionRowView = (connection: IConnectionsItem) => {
  const uploadSpeed = connection.curUpload ?? 0
  const downloadSpeed = connection.curDownload ?? 0

  return {
    id: connection.id,
    host: getConnectionHost(connection),
    process: getConnectionProcess(connection),
    network: connection.metadata.network,
    type: connection.metadata.type,
    chains: formatConnectionChains(connection.chains),
    time: connection.start,
    uploadSpeedText: `${formatConnectionTraffic(uploadSpeed)}/s`,
    downloadSpeedText: `${formatConnectionTraffic(downloadSpeed)}/s`,
    uploadSpeed,
    downloadSpeed,
  } satisfies ConnectionRowView
}

const sameConnectionRowView = (
  left: ConnectionRowView,
  right: ConnectionRowView,
) =>
  left.host === right.host &&
  left.process === right.process &&
  left.network === right.network &&
  left.type === right.type &&
  left.chains === right.chains &&
  left.time === right.time &&
  left.uploadSpeedText === right.uploadSpeedText &&
  left.downloadSpeedText === right.downloadSpeedText &&
  left.uploadSpeed === right.uploadSpeed &&
  left.downloadSpeed === right.downloadSpeed

export const useConnectionRowViews = (connections: IConnectionsItem[]) => {
  const previousRowsRef = useRef(new Map<string, ConnectionRowView>())
  const previousConnectionsRef = useRef(new Map<string, IConnectionsItem>())

  return useMemo(() => {
    const previousRows = previousRowsRef.current
    const previousConnections = previousConnectionsRef.current
    const nextRows = new Map<string, ConnectionRowView>()
    const nextConnections = new Map<string, IConnectionsItem>()
    const rows: ConnectionRowView[] = []

    connections.forEach((connection) => {
      nextConnections.set(connection.id, connection)

      const previousRow = previousRows.get(connection.id)
      const previousConnection = previousConnections.get(connection.id)

      let row: ConnectionRowView
      if (previousRow && previousConnection === connection) {
        row = previousRow
      } else {
        const nextRow = createConnectionRowView(connection)
        row =
          previousRow && sameConnectionRowView(previousRow, nextRow)
            ? previousRow
            : nextRow
      }

      nextRows.set(connection.id, row)
      rows.push(row)
    })

    previousRowsRef.current = nextRows
    previousConnectionsRef.current = nextConnections
    return rows
  }, [connections])
}
