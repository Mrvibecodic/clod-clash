import { WarningAmberRounded } from '@mui/icons-material'
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
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { TINT } from '@/pages/_theme'
import { ensureTunReady } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

export const TunStatus = () => {
  const { t } = useTranslation()
  const { patchVerge } = useVerge()
  const { tunBroken, mutateTunState } = useTunState()
  const { mutateSystemState } = useSystemState()
  const [busy, setBusy] = useState(false)

  const fix = useLockFn(async () => {
    setBusy(true)
    try {
      const ready = await ensureTunReady()
      await mutateSystemState()
      if (!ready) {
        showNotice.error(
          'settings.sections.proxyControl.tooltips.tunUnavailable',
        )
        return
      }
      await patchVerge({ enable_tun_mode: true })
    } catch (error) {
      showNotice.error(error)
    } finally {
      setBusy(false)
      await mutateTunState()
    }
  })

  if (busy) {
    return (
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.5 }}>
        <CircularProgress size={14} />
        <Typography variant="caption" color="text.secondary">
          {t('home.components.tunStatus.settingUp')}
        </Typography>
      </Stack>
    )
  }

  if (!tunBroken) return null

  return (
    <Stack
      direction="row"
      sx={{
        alignItems: 'center',
        gap: 1,
        my: 0.25,
        px: 1.25,
        py: 0.75,
        borderRadius: '12px',
        bgcolor: (theme) => alpha(theme.palette.warning.main, TINT.base),
        border: (theme) =>
          `1px solid ${alpha(theme.palette.warning.main, TINT.edge)}`,
      }}
    >
      <WarningAmberRounded sx={{ fontSize: 16, color: 'warning.main' }} />
      <Typography
        variant="caption"
        color="text.primary"
        sx={{ flex: 1, minWidth: 0 }}
      >
        {t('home.components.tunStatus.broken')}
      </Typography>
      <Button size="small" sx={{ flex: 'none' }} onClick={() => void fix()}>
        {t('home.components.tunStatus.fix')}
      </Button>
    </Stack>
  )
}
