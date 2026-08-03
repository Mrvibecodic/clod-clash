import { WarningAmberRounded } from '@mui/icons-material'
import { Button, CircularProgress, Stack, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useSystemState } from '@/hooks/use-system-state'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { ensureTunReady } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

/**
 * clod:tun-ready — единственное место, где пользователю объясняют, что с TUN
 * что-то не так, и дают это починить одной кнопкой.
 *
 * Тост о неудаче исчезает через несколько секунд, а окно с запросом прав
 * закрывает собой приложение, поэтому обе ситуации нужны в интерфейсе, а не
 * только в уведомлениях:
 *   * пока ставится служба — строка «подтвердите запрос системы» видна за
 *     системным окном и объясняет, кто его показал;
 *   * если ядро не смогло поднять устройство — строка остаётся висеть с
 *     кнопкой «Настроить», а не пропадает вместе с тостом.
 *
 * Когда TUN работает или не запрошен вовсе — компонент не рисует ничего.
 */
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
      // Повторная подача той же настройки снимает сессионное подавление и
      // переводит ядро на службу — это и есть «попробовать ещё раз».
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
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1, py: 0.5 }}>
      <WarningAmberRounded sx={{ fontSize: 16, color: 'warning.main' }} />
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ flex: 1, minWidth: 0 }}
      >
        {t('home.components.tunStatus.broken')}
      </Typography>
      <Button size="small" onClick={() => void fix()}>
        {t('home.components.tunStatus.fix')}
      </Button>
    </Stack>
  )
}
