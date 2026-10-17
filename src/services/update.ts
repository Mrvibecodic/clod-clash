import { Update } from '@tauri-apps/plugin-updater'

import { checkAppUpdate } from '@/services/cmds'

/**
 * Найденное обновление. Версию и канал (стабильный / пре-релизы) уже отобрал
 * бэкенд (`check_update_with_fallback`): более новая версия, манифест своего
 * канала.
 */
export const checkUpdateSafe = async (): Promise<Update | null> => {
  const metadata = await checkAppUpdate()
  return metadata ? new Update(metadata) : null
}
