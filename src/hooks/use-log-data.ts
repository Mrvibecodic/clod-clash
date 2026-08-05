import dayjs from 'dayjs'
import { useEffect, useRef } from 'react'
import { MihomoWebSocket, type LogLevel } from 'tauri-plugin-mihomo-api'

import { getClashLogs } from '@/services/cmds'
import { setCacheData } from '@/services/query-client'

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

/**
 * clod: `enabled` — это видимость окна, как у `useTrafficData`.
 *
 * Гейтом раньше был только флаг «писать логи» из localStorage: окно уезжало в
 * трей, а стрим оставался жить сутками, и каждые 50 мс флаш копировал массив на
 * тысячу элементов и перерисовывал экран, которого никто не видит. Свёрнутое
 * окно новых строк не принимает; уже накопленные не теряются — ключ подписки
 * обнуляется, но данные лежат в кэше по прежнему ключу (см. `responseCacheKey`
 * в `use-mihomo-ws-subscription`), и на возврате стрим продолжает тот же буфер.
 */
export const useLogData = (options?: { enabled?: boolean }) => {
  const enabled = options?.enabled ?? true
  const [clashLog] = useClashLog()
  const enableLog = clashLog.enable && enabled
  const logLevel = clashLog.logLevel.toUpperCase() as LogLevel
  const allowedTypes = LOG_LEVEL_FILTERS[logLevel] ?? DEFAULT_LOG_TYPES
  const hasLoadedInitialLogsRef = useRef(false)

  const { response, refresh, subscriptionCacheKey } = useMihomoWsSubscription<
    ILogItem[]
  >({
    storageKey: 'mihomo_logs_date',
    buildSubscriptKey: (date) => (enableLog ? `getClashLog-${date}` : null),
    fallbackData: [],
    connect: () => MihomoWebSocket.connect_logs(logLevel),
    setupHandlers: ({ next, scheduleReconnect, isMounted }) => {
      let flushTimer: ReturnType<typeof setTimeout> | null = null
      const buffer: ILogItem[] = []
      let flushTimeStr: string | null = null

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
        flushTimeStr = null
        next(null, (current) => appendLogs(current, pendingLogs))
        flushTimer = null
      }

      return {
        handleMessage: (data) => {
          if (data.startsWith('Websocket error')) {
            next(data)
            void scheduleReconnect()
            return
          }

          try {
            const parsed = JSON.parse(data) as ILogItem
            if (
              allowedTypes.length > 0 &&
              !allowedTypes.includes(parsed.type)
            ) {
              return
            }
            if (flushTimeStr === null) {
              flushTimeStr = dayjs().format('MM-DD HH:mm:ss')
            }
            parsed.time = flushTimeStr
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
          if (hasLoadedInitialLogsRef.current) {
            return
          }
          const logs = await getClashLogs()
          hasLoadedInitialLogsRef.current = true
          if (isMounted()) {
            next(null, (current) => {
              if (!current || current.length === 0) {
                return clampLogs(filterLogsByLevel(logs, allowedTypes))
              }
              return current
            })
          }
        },
        cleanup: clearFlushTimer,
      }
    },
  })

  const previousLogLevelRef = useRef<LogLevel | undefined>(logLevel)

  useEffect(() => {
    if (!logLevel) {
      previousLogLevelRef.current = logLevel ?? undefined
      return
    }

    if (previousLogLevelRef.current === logLevel) {
      return
    }

    previousLogLevelRef.current = logLevel
    hasLoadedInitialLogsRef.current = false
    refresh()
  }, [logLevel, refresh])

  const refreshGetClashLog = (clear = false) => {
    if (clear) {
      if (subscriptionCacheKey) {
        setCacheData<ILogItem[]>([subscriptionCacheKey], [])
      }
    } else {
      hasLoadedInitialLogsRef.current = false
      refresh()
    }
  }

  return { response, refreshGetClashLog }
}
