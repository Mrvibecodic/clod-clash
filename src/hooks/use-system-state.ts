import { getRunningMode, isServiceAvailable } from '@/services/cmds'
import { useQuery } from '@/services/query-client'

import { useVisibility } from './use-visibility'

type RunningMode = 'Sidecar' | 'Service' | 'NotRunning' | 'Starting'

interface SystemState {
  runningMode: RunningMode
  isServiceOk: boolean
}

const defaultSystemState = {
  runningMode: 'Sidecar',
  isServiceOk: false,
} as SystemState

/**
 * Пользовательский hook для получения состояния работы системы
 * Включает режим работы и доступность системной службы
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

  const { data: systemState = defaultSystemState, refetch: mutateSystemState } =
    useQuery({
      queryKey: ['getSystemState'],
      queryFn: async () => {
        const [runningMode, isServiceOk] = await Promise.all([
          getRunningMode(),
          isServiceAvailable(),
        ])
        return { runningMode, isServiceOk } as SystemState
      },
      refetchInterval: pageVisible ? 30000 : false,
      refetchOnWindowFocus: true,
      refetchOnReconnect: true,
    })

  const isCoreDown = systemState.runningMode === 'NotRunning'

  return {
    isServiceOk: systemState.isServiceOk,
    isCoreDown,
    mutateSystemState,
  }
}
