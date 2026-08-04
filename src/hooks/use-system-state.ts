import { getRunningMode, isAdmin, isServiceAvailable } from '@/services/cmds'
import { useQuery } from '@/services/query-client'

import { useVisibility } from './use-visibility'

interface SystemState {
  runningMode: 'Sidecar' | 'Service'
  isAdminMode: boolean
  isServiceOk: boolean
}

const defaultSystemState = {
  runningMode: 'Sidecar',
  isAdminMode: false,
  isServiceOk: false,
} as SystemState

/**
 * Пользовательский hook для получения состояния работы системы
 * Включает режим работы, статус администратора, доступность системной службы
 *
 * clod:tun-ready — раньше этот хук ещё и выключал TUN, если очередная проверка
 * не нашла службу. Проверка одноразовая (в Rust это одна попытка `connect()`),
 * грейс снимался по таймеру раньше, чем бэкенд успевал дождаться службы, а сам
 * хук живёт в семи местах — получались параллельные `patchVerge` и одинаковые
 * тосты. Теперь состоянием TUN распоряжается бэкенд: он подавляет режим на
 * сессию, если ядро не смогло поднять устройство, и присылает уведомление.
 * Хук снова только читает.
 */
export function useSystemState() {
  const pageVisible = useVisibility()

  const {
    data: systemState = defaultSystemState,
    refetch: mutateSystemState,
    isLoading,
  } = useQuery({
    queryKey: ['getSystemState'],
    queryFn: async () => {
      const [runningMode, isAdminMode, isServiceOk] = await Promise.all([
        getRunningMode(),
        isAdmin(),
        isServiceAvailable(),
      ])
      return { runningMode, isAdminMode, isServiceOk } as SystemState
    },
    refetchInterval: pageVisible ? 30000 : false,
    refetchOnWindowFocus: true,
    refetchOnReconnect: true,
  })

  const isSidecarMode = systemState.runningMode === 'Sidecar'
  const isServiceMode = systemState.runningMode === 'Service'
  const isTunModeAvailable = systemState.isAdminMode || systemState.isServiceOk

  return {
    runningMode: systemState.runningMode,
    isAdminMode: systemState.isAdminMode,
    isServiceOk: systemState.isServiceOk,
    isSidecarMode,
    isServiceMode,
    isTunModeAvailable,
    mutateSystemState,
    isLoading,
  }
}
