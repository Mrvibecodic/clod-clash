import { useCallback, useMemo, useRef } from 'react'
import {
  closeConnection,
  getConnections,
  selectNodeForGroup,
} from 'tauri-plugin-mihomo-api'

import { useProfiles } from '@/hooks/use-profiles'
import { useVerge } from '@/hooks/use-verge'
import { syncTrayProxySelection } from '@/services/cmds'
import { debugLog } from '@/utils/debug'

// Очистка кэшированных соединений
const cleanupConnections = async (previousProxy: string) => {
  try {
    const { connections } = await getConnections()
    const cleanupPromises = (connections ?? [])
      .filter((conn) => conn.chains.includes(previousProxy))
      .map((conn) => closeConnection(conn.id))

    if (cleanupPromises.length > 0) {
      await Promise.allSettled(cleanupPromises)
      debugLog(`[ProxySelection] Очищено соединений: ${cleanupPromises.length}`)
    }
  } catch (error) {
    console.warn('[ProxySelection] Не удалось очистить соединения:', error)
  }
}

interface ProxySelectionOptions {
  onSuccess?: () => void
  onError?: (error: any) => void
  enableConnectionCleanup?: boolean
}

interface ProxyChangeRequest {
  groupName: string
  proxyName: string
  previousProxy?: string
  skipConfigSave: boolean
}

// Хук выбора прокси
export const useProxySelection = (options: ProxySelectionOptions = {}) => {
  const { current, patchCurrent } = useProfiles()
  const { verge } = useVerge()
  const pendingRequestRef = useRef<ProxyChangeRequest | null>(null)
  const isProcessingRef = useRef(false)

  const { onSuccess, onError, enableConnectionCleanup = true } = options

  // Кэш
  const config = useMemo(
    () => ({
      autoCloseConnection: verge?.auto_close_connection ?? false,
      enableConnectionCleanup,
    }),
    [verge?.auto_close_connection, enableConnectionCleanup],
  )

  // Переключение узла
  const syncTraySelection = useCallback(() => {
    syncTrayProxySelection().catch((error) => {
      console.error(
        '[ProxySelection] Не удалось синхронизировать состояние трея:',
        error,
      )
    })
  }, [])

  const persistSelection = useCallback(
    (groupName: string, proxyName: string, skipConfigSave: boolean) => {
      if (!current || skipConfigSave) return

      const selected = current.selected ? [...current.selected] : []
      const index = selected.findIndex((item) => item.name === groupName)

      if (index < 0) {
        selected.push({ name: groupName, now: proxyName })
      } else {
        selected[index] = { name: groupName, now: proxyName }
      }

      patchCurrent({ selected }).catch((error) => {
        console.error(
          '[ProxySelection] Не удалось сохранить выбор прокси:',
          error,
        )
      })
    },
    [current, patchCurrent],
  )

  const executeChange = useCallback(
    async (request: ProxyChangeRequest) => {
      const { groupName, proxyName, previousProxy, skipConfigSave } = request
      debugLog(
        `[ProxySelection] Переключение прокси: ${groupName} -> ${proxyName}`,
      )

      try {
        await selectNodeForGroup(groupName, proxyName)
        onSuccess?.()
        syncTraySelection()
        persistSelection(groupName, proxyName, skipConfigSave)
        debugLog(
          `[ProxySelection] Прокси и состояние синхронизированы: ${groupName} -> ${proxyName}`,
        )

        if (
          config.enableConnectionCleanup &&
          config.autoCloseConnection &&
          previousProxy
        ) {
          void cleanupConnections(previousProxy)
        }
      } catch (error) {
        console.error(
          `[ProxySelection] Не удалось переключить прокси: ${groupName} -> ${proxyName}`,
          error,
        )
        onError?.(error)
      }
    },
    [config, onError, onSuccess, persistSelection, syncTraySelection],
  )

  const flushChangeQueue = useCallback(async () => {
    if (isProcessingRef.current) return
    isProcessingRef.current = true

    try {
      while (pendingRequestRef.current) {
        const request = pendingRequestRef.current
        pendingRequestRef.current = null
        await executeChange(request)
      }
    } finally {
      isProcessingRef.current = false
      if (pendingRequestRef.current) {
        void flushChangeQueue()
      }
    }
  }, [executeChange])

  const changeProxy = useCallback(
    (
      groupName: string,
      proxyName: string,
      previousProxy?: string,
      skipConfigSave: boolean = false,
    ) => {
      pendingRequestRef.current = {
        groupName,
        proxyName,
        previousProxy,
        skipConfigSave,
      }
      void flushChangeQueue()
    },
    [flushChangeQueue],
  )

  const handleSelectChange = useCallback(
    (
      groupName: string,
      previousProxy?: string,
      skipConfigSave: boolean = false,
    ) =>
      (event: { target: { value: string } }) => {
        const newProxy = event.target.value
        changeProxy(groupName, newProxy, previousProxy, skipConfigSave)
      },
    [changeProxy],
  )

  const handleProxyGroupChange = useCallback(
    (group: { name: string; now?: string }, proxy: { name: string }) => {
      changeProxy(group.name, proxy.name, group.now)
    },
    [changeProxy],
  )

  return {
    changeProxy,
    handleSelectChange,
    handleProxyGroupChange,
  }
}
