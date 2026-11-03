import { ErrorOutlineRounded } from '@mui/icons-material'
import { CircularProgress } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useSystemState } from '@/hooks/use-system-state'
import { getRunningMode, restartCore } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

import { StatusStrip } from './status-strip'

export const CoreStatus = () => {
  const { t } = useTranslation()
  const { isCoreDown, mutateSystemState } = useSystemState()
  const [busy, setBusy] = useState(false)

  const start = useLockFn(async () => {
    setBusy(true)
    try {
      if ((await getRunningMode()) === 'NotRunning') {
        await restartCore()
      }
    } catch (error) {
      showNotice.error(error)
    } finally {
      setBusy(false)
      await mutateSystemState()
    }
  })

  if (!isCoreDown && !busy) return null

  return (
    <StatusStrip
      tone="error"
      icon={
        busy ? (
          <CircularProgress size={14} />
        ) : (
          <ErrorOutlineRounded sx={{ fontSize: 16, color: 'error.main' }} />
        )
      }
      text={t(
        busy
          ? 'home.components.coreStatus.starting'
          : 'home.components.coreStatus.stopped',
      )}
      action={{
        label: t('home.components.coreStatus.start'),
        onClick: () => void start(),
        disabled: busy,
      }}
    />
  )
}
