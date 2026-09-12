import { useRef, useState } from 'react'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { useVerge } from '@/hooks/use-verge'
import { useSystemData } from '@/providers/app-data-context'
import { getAutotemProxy } from '@/services/cmds'
import { revalidateQueries, useQuery } from '@/services/query-client'
import { isProxyServerAt } from '@/utils/ports'

// Единая логика определения состояния системного прокси
export const useSystemProxyState = () => {
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { sysproxy } = useSystemData()
  const { data: autoproxy } = useQuery({
    queryKey: ['getAutotemProxy'],
    queryFn: getAutotemProxy,
    refetchOnWindowFocus: true,
    refetchOnReconnect: true,
  })

  const { enable_system_proxy, proxy_auto_config, proxy_host } = verge ?? {}

  // Фактическое состояние ОС: enable + адрес совпадает с этим приложением
  const indicator = (() => {
    const host = proxy_host || '127.0.0.1'
    if (proxy_auto_config) {
      if (!autoproxy?.enable) return false
      const pacPort = import.meta.env.DEV ? 11233 : 33331
      return autoproxy.url === `http://${host}:${pacPort}/commands/pac`
    } else {
      if (!sysproxy?.enable) return false
      return isProxyServerAt(sysproxy.server, host, sysproxy.current_port)
    }
  })()

  // Режим "применяется только последнее": при быстрых последовательных кликах выполняется только конечное состояние
  const pendingRef = useRef<boolean | null>(null)
  const busyRef = useRef(false)
  // Бэкенд успевает спросить ядро про порт и записать настройки ОС — это
  // заметная пауза, и переключатель обязан показывать, что работа идёт, а не
  // выглядеть проигнорированным.
  const [busy, setBusy] = useState(false)

  const toggleSystemProxy = async (enabled: boolean) => {
    mutateVerge(
      (prev) => (prev ? { ...prev, enable_system_proxy: enabled } : prev),
      false,
    )
    pendingRef.current = enabled

    if (busyRef.current) return
    busyRef.current = true
    setBusy(true)

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
      try {
        await revalidateQueries([
          ['getVergeConfig'],
          ['getSystemProxy'],
          ['getAutotemProxy'],
        ])
      } finally {
        setBusy(false)
      }
    }
  }

  const invalidateProxyState = () =>
    revalidateQueries([['getSystemProxy'], ['getAutotemProxy']])

  return {
    indicator,
    configState: enable_system_proxy ?? false,
    busy,
    toggleSystemProxy,
    invalidateProxyState,
  }
}
