import { WarningAmberRounded } from '@mui/icons-material'
import { Button, CircularProgress, Stack, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useTunState } from '@/hooks/use-tun-state'
import { fixCoreFirewall, getCoreFirewallOk } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useQuery } from '@/services/query-client'
import getSystem from '@/utils/get-system'

const OS = getSystem()
const LISTENING_STACKS = ['system', 'mixed']

export const FirewallStatus = () => {
  const { t } = useTranslation()
  const { tunDesired, tunRuntimeStack } = useTunState()
  const relevant =
    OS === 'windows' &&
    tunDesired &&
    LISTENING_STACKS.includes((tunRuntimeStack ?? '').toLowerCase())

  const { data: allowed, refetch } = useQuery({
    queryKey: ['getCoreFirewallOk'],
    queryFn: getCoreFirewallOk,
    enabled: relevant,
    staleTime: 30000,
    refetchOnWindowFocus: true,
  })
  const [busy, setBusy] = useState(false)

  const fix = useLockFn(async () => {
    setBusy(true)
    try {
      const fixed = await fixCoreFirewall()
      if (fixed === false) {
        showNotice.error('home.components.firewallStatus.failed')
      }
    } catch (error) {
      showNotice.error(error)
    } finally {
      setBusy(false)
      void refetch()
    }
  })

  if (!relevant) return null

  if (busy) {
    return (
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.5 }}>
        <CircularProgress size={14} />
        <Typography variant="caption" color="text.secondary">
          {t('home.components.firewallStatus.fixing')}
        </Typography>
      </Stack>
    )
  }

  if (allowed !== false) return null

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.5 }}>
      <WarningAmberRounded sx={{ fontSize: 16, color: 'warning.main' }} />
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ flex: 1, minWidth: 0 }}
      >
        {t('home.components.firewallStatus.blocked')}
      </Typography>
      <Button size="small" onClick={() => void fix()}>
        {t('home.components.firewallStatus.fix')}
      </Button>
    </Stack>
  )
}
