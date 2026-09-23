import { useCallback, useMemo, useSyncExternalStore } from 'react'
import { MihomoWebSocket } from 'tauri-plugin-mihomo-api'

import { isWsErrorMessage } from '@/utils/ws-error'

const MAX_CLOSED_CONNS_NUM = 500
const CONNECTION_UPDATE_THROTTLE_MS = 500
const CONNECTION_RECONNECT_DELAY_MS = 1_000
/**
 * clod:Р10-77 — «скорость» соединения считалась разницей двух соседних
 * снимков и подписывалась «в секунду», хотя снимки идут не строго раз в
 * секунду, а после часа в трее (сокет закрыт, снимков нет) первая разница —
 * это трафик за весь час. Прирост делится на фактический интервал, а снимок
 * после паузы длиннее этого порога считается первым: скорость по нему — ноль.
 */
const CONNECTION_RATE_GAP_MS = 5_000
const CONNECTION_STALE_MS = 5_000

type ConnectionMetadata = IConnectionsItem['metadata']
type ConnectionListener = () => void

const metadataValue = (value?: string) => value || ''

const initConnData: ConnectionMonitorData = {
  uploadTotal: 0,
  downloadTotal: 0,
  activeConnections: [],
  closedConnections: [],
}

interface ConnectionMonitorData {
  uploadTotal: number
  downloadTotal: number
  activeConnections: IConnectionsItem[]
  closedConnections: IConnectionsItem[]
}

let connectionData: ConnectionMonitorData = initConnData
let connectionSocket: MihomoWebSocket | null = null
let connectionConnecting = false
let reconnectTimer: ReturnType<typeof setTimeout> | null = null
let staleTimer: ReturnType<typeof setTimeout> | null = null
let droppedAsDead = false
let flushTimer: ReturnType<typeof setTimeout> | null = null
let pendingMessageData: string | null = null
let lastFlushAt = 0

const connectionListeners = new Set<ConnectionListener>()

const notifyConnectionListeners = () => {
  connectionListeners.forEach((listener) => listener())
}

const hasConnectionSubscribers = () => connectionListeners.size > 0

const sameMetadata = (left: ConnectionMetadata, right: ConnectionMetadata) =>
  metadataValue(left.network) === metadataValue(right.network) &&
  metadataValue(left.type) === metadataValue(right.type) &&
  metadataValue(left.host) === metadataValue(right.host) &&
  metadataValue(left.sourceIP) === metadataValue(right.sourceIP) &&
  metadataValue(left.sourcePort) === metadataValue(right.sourcePort) &&
  metadataValue(left.destinationPort) ===
    metadataValue(right.destinationPort) &&
  metadataValue(left.destinationIP) === metadataValue(right.destinationIP) &&
  metadataValue(left.remoteDestination) ===
    metadataValue(right.remoteDestination) &&
  metadataValue(left.process) === metadataValue(right.process) &&
  metadataValue(left.processPath) === metadataValue(right.processPath)

const normalizeMetadata = (
  metadata: ConnectionMetadata,
  previous?: ConnectionMetadata,
): ConnectionMetadata => {
  if (previous && sameMetadata(previous, metadata)) return previous

  return {
    network: metadata.network || '',
    type: metadata.type || '',
    host: metadata.host || '',
    sourceIP: metadata.sourceIP || '',
    sourcePort: metadata.sourcePort || '',
    destinationPort: metadata.destinationPort || '',
    destinationIP: metadata.destinationIP || '',
    remoteDestination: metadata.remoteDestination || '',
    process: metadata.process || '',
    processPath: metadata.processPath || '',
  }
}

const sameChains = (left: string[], right: string[]) => {
  if (left.length !== right.length) return false
  for (let i = 0; i < left.length; i++) {
    if (left[i] !== right[i]) return false
  }
  return true
}

const normalizeChains = (chains: string[], previous?: string[]) => {
  if (previous && sameChains(previous, chains)) return previous
  return chains.slice()
}

/**
 * Байты в секунду по приросту за интервал; `null` — интервал неизвестен.
 * Ядро шлёт снимок раз в секунду, а время прихода в окно плавает: два
 * снимка, пришедшие разом после подвисания интерфейса, давали бы при делении
 * на полсекунды удвоенную скорость — поэтому интервал короче секунды не
 * берётся.
 */
const bytesPerSecond = (delta: number, elapsedMs: number | null) =>
  elapsedMs === null
    ? 0
    : Math.round((delta * 1000) / Math.max(elapsedMs, 1000))

const normalizeConnection = (
  connection: IConnectionsItem,
  previous: IConnectionsItem | undefined,
  elapsedMs: number | null,
): IConnectionsItem => {
  const metadata = normalizeMetadata(connection.metadata, previous?.metadata)
  const chains = normalizeChains(connection.chains || [], previous?.chains)
  const upload = connection.upload ?? 0
  const download = connection.download ?? 0
  const curUpload = previous
    ? bytesPerSecond(upload - previous.upload, elapsedMs)
    : 0
  const curDownload = previous
    ? bytesPerSecond(download - previous.download, elapsedMs)
    : 0
  const rule = connection.rule || ''
  const rulePayload = connection.rulePayload || ''
  const start = connection.start || ''

  if (
    previous &&
    previous.metadata === metadata &&
    previous.chains === chains &&
    previous.upload === upload &&
    previous.download === download &&
    previous.curUpload === curUpload &&
    previous.curDownload === curDownload &&
    previous.rule === rule &&
    previous.rulePayload === rulePayload &&
    previous.start === start
  ) {
    return previous
  }

  return {
    id: connection.id,
    metadata,
    upload,
    download,
    start,
    chains,
    rule,
    rulePayload,
    curUpload,
    curDownload,
  }
}

const mergeConnectionSnapshot = (
  payload: IConnections,
  previous: ConnectionMonitorData,
  elapsedMs: number | null,
): ConnectionMonitorData => {
  const nextConnections = payload.connections ?? []
  const previousActive = previous.activeConnections ?? []
  const previousClosed = previous.closedConnections ?? []
  const previousActiveById = new Map<string, IConnectionsItem>()

  for (let i = 0; i < previousActive.length; i++) {
    const previousConnection = previousActive[i]
    previousActiveById.set(previousConnection.id, previousConnection)
  }

  const activeConnections: IConnectionsItem[] = []
  for (let i = 0; i < nextConnections.length; i++) {
    const connection = nextConnections[i]
    const previousConnection = previousActiveById.get(connection.id)
    if (previousConnection) previousActiveById.delete(connection.id)
    activeConnections.push(
      normalizeConnection(connection, previousConnection, elapsedMs),
    )
  }

  if (previousActiveById.size === 0) {
    return {
      uploadTotal: payload.uploadTotal ?? 0,
      downloadTotal: payload.downloadTotal ?? 0,
      activeConnections,
      closedConnections: previousClosed,
    }
  }

  const removedConnectionCount = previousActiveById.size
  const dropFromClosed = Math.max(
    0,
    previousClosed.length + removedConnectionCount - MAX_CLOSED_CONNS_NUM,
  )
  const closedConnections =
    dropFromClosed >= previousClosed.length
      ? []
      : previousClosed.slice(dropFromClosed)

  const keepFromRemoved = MAX_CLOSED_CONNS_NUM - closedConnections.length
  let skipRemoved = Math.max(0, removedConnectionCount - keepFromRemoved)

  for (let i = 0; i < previousActive.length; i++) {
    const connection = previousActive[i]
    if (!previousActiveById.has(connection.id)) continue
    if (skipRemoved > 0) {
      skipRemoved -= 1
      continue
    }
    closedConnections.push(
      connection.curUpload || connection.curDownload
        ? { ...connection, curUpload: 0, curDownload: 0 }
        : connection,
    )
  }

  return {
    uploadTotal: payload.uploadTotal ?? 0,
    downloadTotal: payload.downloadTotal ?? 0,
    activeConnections,
    closedConnections,
  }
}

const flushPendingMessage = () => {
  flushTimer = null
  const messageData = pendingMessageData
  pendingMessageData = null
  if (!messageData || !hasConnectionSubscribers()) return

  let payload: IConnections
  try {
    payload = JSON.parse(messageData) as IConnections
  } catch (err) {
    console.error('[Connections] Failed to parse websocket payload', err)
    return
  }

  const now = Date.now()
  const sincePrevious = now - lastFlushAt
  lastFlushAt = now
  const merged = mergeConnectionSnapshot(
    payload,
    connectionData,
    sincePrevious > 0 && sincePrevious <= CONNECTION_RATE_GAP_MS
      ? sincePrevious
      : null,
  )
  if (droppedAsDead) {
    droppedAsDead = false
    const alive = new Set(merged.activeConnections.map((item) => item.id))
    merged.closedConnections = merged.closedConnections.filter(
      (item) => !alive.has(item.id),
    )
  }
  connectionData = merged
  notifyConnectionListeners()
}

const enqueueConnectionMessage = (messageData: string) => {
  pendingMessageData = messageData
  if (flushTimer) return

  const elapsed = Date.now() - lastFlushAt
  if (elapsed >= CONNECTION_UPDATE_THROTTLE_MS) {
    flushPendingMessage()
    return
  }

  flushTimer = window.setTimeout(
    flushPendingMessage,
    CONNECTION_UPDATE_THROTTLE_MS - elapsed,
  )
}

const clearReconnectTimer = () => {
  if (!reconnectTimer) return
  window.clearTimeout(reconnectTimer)
  reconnectTimer = null
}

const clearStaleTimer = () => {
  if (!staleTimer) return
  window.clearTimeout(staleTimer)
  staleTimer = null
}

const closeConnectionSocket = async () => {
  clearStaleTimer()
  const socket = connectionSocket
  connectionSocket = null
  if (!socket) return

  try {
    await socket.close()
  } catch (err) {
    console.warn('Failed to close connection websocket', err)
  }
}

const scheduleReconnect = () => {
  if (!hasConnectionSubscribers()) return
  if (reconnectTimer) return
  reconnectTimer = window.setTimeout(() => {
    reconnectTimer = null
    void connectConnectionSocket()
  }, CONNECTION_RECONNECT_DELAY_MS)
}

async function reconnectConnectionSocket() {
  if (!hasConnectionSubscribers()) return
  await closeConnectionSocket()
  scheduleReconnect()
}

function dropDeadConnectionSocket() {
  pendingMessageData = null
  if (flushTimer) {
    window.clearTimeout(flushTimer)
    flushTimer = null
  }
  if (connectionData.activeConnections.length > 0) {
    connectionData = mergeConnectionSnapshot(
      { uploadTotal: 0, downloadTotal: 0, connections: [] },
      connectionData,
      null,
    )
    droppedAsDead = true
    notifyConnectionListeners()
  }
  void reconnectConnectionSocket()
}

function armStaleTimer(socket: MihomoWebSocket) {
  clearStaleTimer()
  staleTimer = window.setTimeout(() => {
    staleTimer = null
    if (connectionSocket === socket) dropDeadConnectionSocket()
  }, CONNECTION_STALE_MS)
}

async function connectConnectionSocket() {
  if (connectionSocket || connectionConnecting) return
  if (!hasConnectionSubscribers()) return

  clearReconnectTimer()
  connectionConnecting = true

  try {
    const socket = await MihomoWebSocket.connect_connections()
    if (!hasConnectionSubscribers()) {
      await socket.close()
      return
    }
    connectionSocket = socket
    armStaleTimer(socket)
    socket.addListener((message) => {
      if (connectionSocket !== socket) return
      if (message.type !== 'Text') return
      if (isWsErrorMessage(message.data)) {
        dropDeadConnectionSocket()
        return
      }

      armStaleTimer(socket)
      enqueueConnectionMessage(message.data)
    })
  } catch {
    scheduleReconnect()
  } finally {
    connectionConnecting = false
  }
}

const startConnectionMonitor = () => {
  void connectConnectionSocket()
}

const stopConnectionMonitorIfIdle = () => {
  if (hasConnectionSubscribers()) return

  clearReconnectTimer()
  pendingMessageData = null
  if (flushTimer) {
    window.clearTimeout(flushTimer)
    flushTimer = null
  }
  void closeConnectionSocket()
}

const getConnectionSnapshot = () => connectionData

const subscribeConnectionData = (listener: ConnectionListener) => {
  connectionListeners.add(listener)
  startConnectionMonitor()
  return () => {
    connectionListeners.delete(listener)
    stopConnectionMonitorIfIdle()
  }
}

const refreshConnectionData = () => {
  pendingMessageData = null
  if (flushTimer) {
    window.clearTimeout(flushTimer)
    flushTimer = null
  }

  void reconnectConnectionSocket()
}

const clearClosedConnectionData = () => {
  if (connectionData.closedConnections.length === 0) return
  connectionData = {
    ...connectionData,
    closedConnections: [],
  }
  notifyConnectionListeners()
}

export const useConnectionData = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled ?? true
  const subscribe = useCallback(
    (listener: ConnectionListener) =>
      enabled ? subscribeConnectionData(listener) : () => {},
    [enabled],
  )
  const data = useSyncExternalStore(
    subscribe,
    getConnectionSnapshot,
    getConnectionSnapshot,
  )
  const response = useMemo(() => ({ data }), [data])
  const refreshGetClashConnection = useCallback(() => {
    refreshConnectionData()
  }, [])
  const clearClosedConnections = useCallback(() => {
    clearClosedConnectionData()
  }, [])

  return {
    response,
    refreshGetClashConnection,
    clearClosedConnections,
  }
}
