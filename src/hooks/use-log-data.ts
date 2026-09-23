import dayjs from 'dayjs'
import { useEffect, useRef } from 'react'
import { MihomoWebSocket, type LogLevel } from 'tauri-plugin-mihomo-api'

import { getClashLogs } from '@/services/cmds'
import { setCacheData } from '@/services/query-client'

import { useRuntimeConfig } from './use-clash'
import { useClashLog } from './use-clash-log'
import { useMihomoWsSubscription } from './use-mihomo-ws-subscription'

const MAX_LOG_NUM = 1000
const FLUSH_DELAY_MS = 50
type LogType = ILogItem['type']

const DEFAULT_LOG_TYPES: LogType[] = ['debug', 'info', 'warning', 'error']
const LOG_LEVEL_FILTERS: Record<LogLevel, LogType[]> = {
  DEBUG: DEFAULT_LOG_TYPES,
  INFO: ['info', 'warning', 'error'],
  WARNING: ['warning', 'error'],
  ERROR: ['error'],
  SILENT: [],
}

const initialLogsTaken = new Set<string>()

const coreLogLevel = (value: unknown): LogLevel => {
  const level = typeof value === 'string' ? value.toUpperCase() : 'INFO'
  if (level === 'WARN') return 'WARNING'
  return level in LOG_LEVEL_FILTERS ? (level as LogLevel) : 'INFO'
}

const clampLogs = (logs: ILogItem[]): ILogItem[] =>
  logs.length > MAX_LOG_NUM ? logs.slice(-MAX_LOG_NUM) : logs

const filterLogsByLevel = (
  logs: ILogItem[],
  allowedTypes: LogType[],
): ILogItem[] => {
  if (allowedTypes.length === 0) return []
  if (allowedTypes.length === DEFAULT_LOG_TYPES.length) return logs
  return logs.filter((log) => allowedTypes.includes(log.type))
}

const appendLogs = (
  current: ILogItem[] | undefined,
  incoming: ILogItem[],
): ILogItem[] => {
  const base = current ?? []
  const total = base.length + incoming.length
  if (total <= MAX_LOG_NUM) return base.concat(incoming)
  const dropFromBase = total - MAX_LOG_NUM
  if (dropFromBase >= base.length) {
    return incoming.slice(incoming.length - MAX_LOG_NUM)
  }
  return base.slice(dropFromBase).concat(incoming)
}

export const useLogData = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled ?? true
  const [clashLog] = useClashLog()
  const { data: runtime } = useRuntimeConfig()
  const logLevel = runtime ? coreLogLevel(runtime['log-level']) : undefined
  const enableLog = clashLog.enable && enabled && logLevel !== undefined
  const allowedTypes = logLevel
    ? LOG_LEVEL_FILTERS[logLevel]
    : DEFAULT_LOG_TYPES

  const { response, refresh, subscriptionCacheKey } = useMihomoWsSubscription<
    ILogItem[]
  >({
    storageKey: 'mihomo_logs_date',
    buildSubscriptKey: (date) => (enableLog ? `getClashLog-${date}` : null),
    fallbackData: [],
    connect: () => MihomoWebSocket.connect_logs(logLevel ?? 'INFO'),
    setupHandlers: ({ next, isMounted, cacheKey }) => {
      let flushTimer: ReturnType<typeof setTimeout> | null = null
      const buffer: ILogItem[] = []

      const clearFlushTimer = () => {
        if (flushTimer) {
          clearTimeout(flushTimer)
          flushTimer = null
        }
      }

      const flush = () => {
        if (!buffer.length || !isMounted()) {
          flushTimer = null
          return
        }
        const pendingLogs = buffer.splice(0, buffer.length)
        next(null, (current) => appendLogs(current, pendingLogs))
        flushTimer = null
      }

      return {
        handleMessage: (data) => {
          try {
            const parsed = JSON.parse(data) as ILogItem
            if (
              allowedTypes.length > 0 &&
              !allowedTypes.includes(parsed.type)
            ) {
              return
            }
            parsed.time = dayjs().format('MM-DD HH:mm:ss')
            buffer.push(parsed)
            if (buffer.length > MAX_LOG_NUM) {
              buffer.splice(0, buffer.length - MAX_LOG_NUM)
            }
            if (!flushTimer) {
              flushTimer = setTimeout(flush, FLUSH_DELAY_MS)
            }
          } catch (error) {
            next(error)
          }
        },
        async onConnected() {
          if (initialLogsTaken.has(cacheKey)) {
            return
          }
          const logs = await getClashLogs()
          if (!isMounted() || initialLogsTaken.has(cacheKey)) return
          initialLogsTaken.add(cacheKey)
          next(null, (current) => {
            if (!current || current.length === 0) {
              return clampLogs(filterLogsByLevel(logs, allowedTypes))
            }
            return current
          })
        },
        cleanup: clearFlushTimer,
      }
    },
  })

  const previousLogLevelRef = useRef<LogLevel | undefined>(logLevel)

  useEffect(() => {
    if (!logLevel || previousLogLevelRef.current === logLevel) return

    const known = previousLogLevelRef.current !== undefined
    previousLogLevelRef.current = logLevel
    if (known) refresh()
  }, [logLevel, refresh])

  const clearLogs = () => {
    if (subscriptionCacheKey) {
      initialLogsTaken.add(subscriptionCacheKey)
      setCacheData<ILogItem[]>([subscriptionCacheKey], [])
    }
  }

  return { response, clearLogs }
}
