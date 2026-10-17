import type { Update } from '@tauri-apps/plugin-updater'

import { useQuery } from '@/services/query-client'
import { checkUpdateSafe } from '@/services/update'

import { useVerge } from './use-verge'

const CHECK_EVERY_MS = 24 * 60 * 60 * 1000
const RETRY_MS = 2 * 60 * 1000

// Ни одного ответа (`undefined`; «обновлений нет» — `null`) — обычно сеть
// ещё не поднялась к автозапуску: спрашиваем снова скоро, а не через сутки.
const nextCheckIn = (info: Update | null | undefined) =>
  info === undefined ? RETRY_MS : CHECK_EVERY_MS

export const useUpdate = () => {
  const { verge } = useVerge()
  const { auto_check_update } = verge || {}

  const shouldCheck = auto_check_update !== false

  const { data: updateInfo } = useQuery({
    queryKey: ['checkUpdate'],
    queryFn: checkUpdateSafe,
    enabled: shouldCheck,
    // Короче повтора: иначе повтор получил бы прежний неудачный ответ.
    staleTime: RETRY_MS / 2,
    refetchInterval: nextCheckIn,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: false,
  })

  return { updateInfo }
}
