import { Typography } from '@mui/material'
import { memo } from 'react'

import { useSessionUptime } from '@/hooks/use-session-uptime'

interface Props {
  /** Цвет состояния «подключено» — таймер стоит на месте надписи о нём. */
  color: string
  /** Что показать, пока бэкенд не вернул начало сессии. */
  fallback: string
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

const ConnectUptimeView = ({ color, fallback }: Props) => {
  const uptime = useSessionUptime(true)

  return (
    <Typography
      variant="subtitle1"
      sx={{
        color,
        fontVariantNumeric: 'tabular-nums',
        letterSpacing: 1,
        fontWeight: 600,
        fontSize: 18,
        lineHeight: '28px',
      }}
    >
      {uptime === undefined ? fallback : formatUptime(uptime)}
    </Typography>
  )
}

export const ConnectUptime = memo(ConnectUptimeView)
