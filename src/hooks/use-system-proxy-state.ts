import { useRef } from 'react'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { useVerge } from '@/hooks/use-verge'
import { useClashConfigData, useSystemData } from '@/providers/app-data-context'
import { getAutotemProxy } from '@/services/cmds'
import { revalidateQueries, useQuery } from '@/services/query-client'

// Единая логика определения состояния системного прокси
export const useSystemProxyState = () => {
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { sysproxy } = useSystemData()
  const { clashConfig } = useClashConfigData()
  const { data: autoproxy } = useQuery({
    queryKey: ['getAutotemProxy'],
    queryFn: getAutotemProxy,
    refetchOnWindowFocus: true,
    refetchOnReconnect: true,
  })

  const {
    enable_system_proxy,
    proxy_auto_config,
    proxy_host,
    verge_mixed_port,
  } = verge ?? {}

  // Фактическое состояние ОС: enable + адрес совпадает с этим приложением
  const indicator = (() => {
    const host = proxy_host || '127.0.0.1'
    if (proxy_auto_config) {
      if (!autoproxy?.enable) return false
      const pacPort = import.meta.env.DEV ? 11233 : 33331
      return autoproxy.url === `http://${host}:${pacPort}/commands/pac`
    } else {
      if (!sysproxy?.enable) return false
      const port = verge_mixed_port || clashConfig?.mixedPort || 7897
      return sysproxy.server === `${host}:${port}`
    }
  })()

  // Режим "применяется только последнее": при быстрых последовательных кликах выполняется только конечное состояние
  const pendingRef = useRef<boolean | null>(null)
  const busyRef = useRef(false)

  const toggleSystemProxy = async (enabled: boolean) => {
    mutateVerge(
      (prev) => (prev ? { ...prev, enable_system_proxy: enabled } : prev),
      false,
    )
    pendingRef.current = enabled

    if (busyRef.current) return
    busyRef.current = true

    try {
      while (pendingRef.current !== null) {
        const target = pendingRef.current
        pendingRef.current = null
        await patchVerge({ enable_system_proxy: target })
        if (
          !target &&
          verge?.auto_close_connection &&
          !verge?.enable_tun_mode
        ) {
          await closeAllConnections().catch(() => {})
        }
      }
    } finally {
      busyRef.current = false
      await revalidateQueries([
        ['getVergeConfig'],
        ['getSystemProxy'],
        ['getAutotemProxy'],
      ])
    }
  }

  const invalidateProxyState = () =>
    revalidateQueries([['getSystemProxy'], ['getAutotemProxy']])

  return {
    indicator,
    configState: enable_system_proxy ?? false,
    toggleSystemProxy,
    invalidateProxyState,
  }
}
