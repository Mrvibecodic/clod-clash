import { Typography } from '@mui/material'
import { memo } from 'react'

import { useSessionUptime } from '@/hooks/use-session-uptime'

interface Props {
  active: boolean
}

const formatUptime = (seconds: number) => {
  const total = Math.max(0, Math.floor(seconds))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor((total % 3600) / 60)
  const secs = total % 60
  const pad = (value: number) => String(value).padStart(2, '0')
  return hours > 0
    ? `${pad(hours)}:${pad(minutes)}:${pad(secs)}`
    : `${pad(minutes)}:${pad(secs)}`
}

const ConnectUptimeView = ({ active }: Props) => {
  const uptime = useSessionUptime(active)
  const shown = active && uptime !== undefined

  return (
    <Typography
      variant="body1"
      sx={{
        fontVariantNumeric: 'tabular-nums',
        letterSpacing: 1,
        fontWeight: 600,
        minHeight: 24,
        visibility: shown ? 'visible' : 'hidden',
      }}
    >
      {shown ? formatUptime(uptime) : '00:00'}
    </Typography>
  )
}

export const ConnectUptime = memo(ConnectUptimeView)
