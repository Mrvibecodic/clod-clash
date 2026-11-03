import { WarningAmberRounded } from '@mui/icons-material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useTunState } from '@/hooks/use-tun-state'
import { fixCoreFirewall, getCoreFirewallOk } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useQuery } from '@/services/query-client'
import getSystem from '@/utils/get-system'

import { StatusBusyLine, StatusStrip } from './status-strip'

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
    return <StatusBusyLine text={t('home.components.firewallStatus.fixing')} />
  }

  if (allowed !== false) return null

  return (
    <StatusStrip
      tone="warning"
      icon={
        <WarningAmberRounded sx={{ fontSize: 16, color: 'warning.main' }} />
      }
      text={t('home.components.firewallStatus.blocked')}
      action={{
        label: t('home.components.firewallStatus.fix'),
        onClick: () => void fix(),
      }}
    />
  )
}
