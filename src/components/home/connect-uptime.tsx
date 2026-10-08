import PowerSettingsNewRoundedIcon from '@mui/icons-material/PowerSettingsNewRounded'
import { Typography } from '@mui/material'
import { memo } from 'react'

import { useSessionUptime } from '@/hooks/use-session-uptime'
import { sessionTimeText } from '@/utils/date-text'

interface Props {
  /** Размер значка кнопки без таймера. */
  iconSize: number
  compact?: boolean
}

/** Значок над таймером меньше — как на Android (0,34 → 0,30 диаметра). */
const ICON_WITH_TIMER = 0.3 / 0.34

/**
 * clod: значок кнопки подключения и таймер сессии под ним, внутри кнопки —
 * как на Android. Секундный тик перерисовывает только это. Таймер — пока идёт
 * сессия и прошла хотя бы секунда; вслух состояние произносит aria-label
 * кнопки, цифры в него не попадают.
 */
const ConnectUptimeView = ({ iconSize, compact }: Props) => {
  const uptime = useSessionUptime(true)
  const seconds = Math.floor(uptime ?? 0)
  const timed = seconds > 0

  return (
    <>
      <PowerSettingsNewRoundedIcon
        sx={{
          fontSize: timed ? Math.round(iconSize * ICON_WITH_TIMER) : iconSize,
        }}
      />
      {timed ? (
        <Typography
          component="span"
          aria-hidden
          sx={{
            mt: 0.5,
            color: 'inherit',
            fontSize: compact ? 13 : 15,
            lineHeight: compact ? '16px' : '18px',
            fontWeight: 600,
            fontVariantNumeric: 'tabular-nums',
          }}
        >
          {sessionTimeText(seconds)}
        </Typography>
      ) : null}
    </>
  )
}

export const ConnectUptime = memo(ConnectUptimeView)
