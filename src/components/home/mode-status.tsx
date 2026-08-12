import { Button, Tooltip, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useClashConfigData } from '@/providers/app-data-context'

interface Props {
  /** `clod-lock-mode`: the panel forbids changing modes — plain text then. */
  locked?: boolean
  /**
   * clod: the advanced screen shows the connect targets as switches in
   * `QuickActions`, so repeating them here would be the same fact twice —
   * it keeps only the routing mode.
   */
  showTargets?: boolean
}

/**
 * clod: строка «Системный прокси + TUN · По правилам» под кнопкой Connect.
 * В расширенном режиме таргеты показывает карточка быстрых действий, поэтому
 * там от строки остаётся только режим маршрутизации.
 *
 * Показывается в обоих режимах интерфейса: и в расширенном, и в простом
 * пользователь должен видеть, какие таргеты дёргает Connect и какой режим
 * маршрутизации активен. Read-only намеренно: менять — в настройках, а при
 * `clod-lock-mode` не меняется вовсе (остаётся только строка-статус).
 */
export const ModeStatus = ({ locked, showTargets = true }: Props) => {
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

  const text = showTargets ? `${activeTargets} · ${modeLabel}` : modeLabel

  if (locked) {
    // clod:lock-expiry — замок молча забирал управление, и выход из него нигде
    // не назывался: пользователь с умершим доменом панели видел только серую
    // строку. Подсказка называет оба выхода — срок годности замка и удаление
    // подписки.
    return (
      <Tooltip title={t('home.components.modeStatus.lockedHint')}>
        <Typography
          variant="caption"
          color="text.secondary"
          sx={{ cursor: 'help' }}
        >
          {text}
        </Typography>
      </Tooltip>
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
      {text}
    </Button>
  )
}
