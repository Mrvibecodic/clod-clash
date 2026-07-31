import { Button, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useClashConfigData } from '@/providers/app-data-context'

interface Props {
  /** `clod-lock-mode`: the panel forbids changing modes — plain text then. */
  locked?: boolean
}

/**
 * clod: строка «Системный прокси + TUN · Правила» под кнопкой Connect.
 *
 * Показывается в обоих режимах интерфейса: и в расширенном, и в простом
 * пользователь должен видеть, какие таргеты дёргает Connect и какой режим
 * маршрутизации активен. Read-only намеренно: менять — в настройках, а при
 * `clod-lock-mode` не меняется вовсе (остаётся только строка-статус).
 */
export const ModeStatus = ({ locked }: Props) => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { clashConfig } = useClashConfigData()
  const { targetSys, targetTun } = useConnectTargets()

  const clashMode = clashConfig?.mode?.toLowerCase()
  const modeLabel =
    clashMode === 'global' || clashMode === 'direct' || clashMode === 'rule'
      ? t(`home.components.clashMode.labels.${clashMode}`)
      : t('home.components.clashMode.labels.rule')

  const activeTargets = [
    targetSys ? t('settings.components.verge.basic.options.sysproxy') : null,
    targetTun ? t('settings.components.verge.basic.options.tun') : null,
  ]
    .filter(Boolean)
    .join(' + ')

  if (locked) {
    return (
      <Typography variant="caption" color="text.secondary">
        {activeTargets} · {modeLabel}
      </Typography>
    )
  }

  return (
    <Button
      size="small"
      color="inherit"
      // compact: строка-статус не должна раздувать вертикаль простого режима
      sx={{
        color: 'text.secondary',
        textTransform: 'none',
        py: 0.25,
        minHeight: 0,
      }}
      onClick={() => void navigate('/settings')}
    >
      {activeTargets} · {modeLabel}
    </Button>
  )
}
