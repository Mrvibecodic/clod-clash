import { useCallback } from 'react'

import { useVerge } from '@/hooks/use-verge'
import { showNotice } from '@/services/notice-service'

export const useEnableHwid = () => {
  const { patchVerge } = useVerge()
  return useCallback(async () => {
    try {
      await patchVerge({ enable_hwid: true })
      showNotice.success('profiles.components.hwidDialog.enabled')
      return true
    } catch (error) {
      showNotice.error(error instanceof Error ? error.message : String(error))
      return false
    }
  }, [patchVerge])
}
