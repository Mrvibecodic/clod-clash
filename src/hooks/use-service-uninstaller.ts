import { useCallback } from 'react'

import { uninstallService } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import {
  serviceUninstallNotices,
  UNINSTALL_STAGE,
} from '@/utils/service-uninstall'

import { useSystemState } from './use-system-state'

export const useServiceUninstaller = () => {
  const { mutateSystemState } = useSystemState()

  const uninstallServiceAndRestartCore = useCallback(async () => {
    showNotice.info(UNINSTALL_STAGE.uninstalling)
    try {
      for (const notice of serviceUninstallNotices(await uninstallService())) {
        if ('done' in notice) {
          showNotice.success(notice.done)
        } else if (notice.consequence) {
          showNotice.error(notice.consequence, notice.error)
        } else {
          showNotice.error(notice.error)
        }
      }
    } catch (error) {
      showNotice.error(error)
    }
    await mutateSystemState()
  }, [mutateSystemState])

  return { uninstallServiceAndRestartCore }
}
