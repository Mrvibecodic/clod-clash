import { useRef, useState } from 'react'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { useVerge } from '@/hooks/use-verge'
import { useSystemData } from '@/providers/app-data-context'
import { getAutotemProxy } from '@/services/cmds'
import { revalidateQueries, useQuery } from '@/services/query-client'

/** Адрес PAC, который ставит в систему бэкенд (`utils/server.rs`). */
export const PAC_URL = `http://127.0.0.1:${import.meta.env.DEV ? 11233 : 33331}/commands/pac`

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

  const { enable_system_proxy, proxy_auto_config } = verge ?? {}

  // Фактическое состояние ОС; чей прокси в системе, решает бэкенд — тем же
  // правилом, что трей и горячая клавиша.
  const indicator = proxy_auto_config
    ? Boolean(autoproxy?.enable && autoproxy.ours)
    : Boolean(sysproxy?.enable && sysproxy.ours)

  // Режим "применяется только последнее": при быстрых последовательных кликах выполняется только конечное состояние
  const pendingRef = useRef<{ enabled: boolean; remember: boolean } | null>(
    null,
  )
  const busyRef = useRef(false)
  // Бэкенд успевает спросить ядро про порт и записать настройки ОС — это
  // заметная пауза, и переключатель обязан показывать, что работа идёт, а не
  // выглядеть проигнорированным.
  const [busy, setBusy] = useState(false)

  /**
   * `remember` — нажатие самого тумблера: оно же выбор способа для кнопки
   * «Подключить» (`connect_system_proxy`) и пишется той же записью настроек.
   * Кнопка выбор не пишет: она выключает те же флаги при отключении, и одно
   * нажатие «отключиться» стирало бы его.
   */
  const toggleSystemProxy = async (enabled: boolean, remember = false) => {
    mutateVerge((prev) =>
      prev ? { ...prev, enable_system_proxy: enabled } : prev,
    )
    pendingRef.current = { enabled, remember }

    if (busyRef.current) return
    busyRef.current = true
    setBusy(true)

    let failed = true
    try {
      while (pendingRef.current !== null) {
        const { enabled: target, remember: chosen } = pendingRef.current
        pendingRef.current = null
        await patchVerge({
          enable_system_proxy: target,
          ...(chosen && { connect_system_proxy: target }),
        })
        if (
          !target &&
          verge?.auto_close_connection &&
          !verge?.enable_tun_mode
        ) {
          await closeAllConnections().catch(() => {})
        }
      }
      failed = false
    } finally {
      busyRef.current = false
      try {
        // Настройки после удачного цикла уже перечитал `patchVerge`; после
        // отказа в кэше осталось бы оптимистичное значение.
        await revalidateQueries([
          ...(failed ? [['getVergeConfig']] : []),
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
