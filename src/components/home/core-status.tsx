import { ErrorOutlineRounded } from '@mui/icons-material'
import {
  alpha,
  Button,
  CircularProgress,
  Stack,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useSystemState } from '@/hooks/use-system-state'
import { TINT } from '@/pages/_theme'
import { getRunningMode, restartCore } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

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
    <Stack
      direction="row"
      sx={{
        alignSelf: 'center',
        maxWidth: '100%',
        alignItems: 'center',
        gap: 1,
        my: 0.25,
        px: 1.25,
        py: 0.75,
        borderRadius: '12px',
        bgcolor: (theme) => alpha(theme.palette.error.main, TINT.base),
        border: (theme) =>
          `1px solid ${alpha(theme.palette.error.main, TINT.edge)}`,
      }}
    >
      {busy ? (
        <CircularProgress size={14} />
      ) : (
        <ErrorOutlineRounded sx={{ fontSize: 16, color: 'error.main' }} />
      )}
      <Typography
        variant="caption"
        color="text.primary"
        sx={{ flex: 1, minWidth: 0 }}
      >
        {t(
          busy
            ? 'home.components.coreStatus.starting'
            : 'home.components.coreStatus.stopped',
        )}
      </Typography>
      <Button
        size="small"
        sx={{ flex: 'none' }}
        disabled={busy}
        onClick={() => void start()}
      >
        {t('home.components.coreStatus.start')}
      </Button>
    </Stack>
  )
}
