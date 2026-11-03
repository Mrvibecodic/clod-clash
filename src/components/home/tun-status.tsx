import { WarningAmberRounded } from '@mui/icons-material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useSystemState } from '@/hooks/use-system-state'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { ensureTunReady } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { tunFailureKey, tunSetupNotice } from '@/utils/tun-notice'

import { StatusBusyLine, StatusStrip } from './status-strip'

export const TunStatus = () => {
  const { t } = useTranslation()
  const { patchVerge } = useVerge()
  const { tunDesired, tunBroken, tunNoTraffic, tunFailure, mutateTunState } =
    useTunState()
  const { mutateSystemState } = useSystemState()
  const [busy, setBusy] = useState(false)

  const fix = useLockFn(async () => {
    setBusy(true)
    try {
      const ready = await ensureTunReady()
      await mutateSystemState()
      // Причину отказа бэкенд уже показал уведомлением.
      if (!ready) return
      await patchVerge({ enable_tun_mode: true })
    } catch (error) {
      showNotice.error(tunSetupNotice(error))
    } finally {
      setBusy(false)
      await mutateTunState()
    }
  })

  if (busy) {
    return <StatusBusyLine text={t('home.components.tunStatus.settingUp')} />
  }

  const reasonKey = tunDesired ? tunFailureKey(tunFailure) : undefined

  if (!tunBroken && !tunNoTraffic) return null

  return (
    <StatusStrip
      tone="warning"
      icon={
        <WarningAmberRounded sx={{ fontSize: 16, color: 'warning.main' }} />
      }
      text={t(reasonKey ?? 'home.components.tunStatus.broken')}
      action={
        tunBroken
          ? {
              label: t('home.components.tunStatus.fix'),
              onClick: () => void fix(),
            }
          : undefined
      }
    />
  )
}
