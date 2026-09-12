import { useCallback } from 'react'

import { restartCore, stopCore, uninstallService } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { runServiceUninstall } from '@/utils/service-uninstall'

import { useSystemState } from './use-system-state'

export const useServiceUninstaller = () => {
  const { mutateSystemState } = useSystemState()

  const uninstallServiceAndRestartCore = useCallback(async () => {
    await runServiceUninstall(
      {
        stopCore: () => stopCore(),
        uninstallService: () => uninstallService(),
        restartCore: () => restartCore(),
      },
      {
        busy: (key) => {
          showNotice.info(key)
        },
        done: (key) => {
          showNotice.success(key)
        },
        failed: (error) => {
          showNotice.error(error)
        },
      },
    )
    await mutateSystemState()
  }, [mutateSystemState])

  return { uninstallServiceAndRestartCore }
}
