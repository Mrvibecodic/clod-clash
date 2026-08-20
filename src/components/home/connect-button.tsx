import PowerSettingsNewRoundedIcon from '@mui/icons-material/PowerSettingsNewRounded'
import { Box, CircularProgress, Typography, alpha } from '@mui/material'
import { useTranslation } from 'react-i18next'

export type ConnectState =
  | 'off'
  | 'connecting'
  | 'disconnecting'
  | 'on'
  | 'error'

interface Props {
  state: ConnectState
  uptime?: number
  errorText?: string
  disabled?: boolean
  compact?: boolean
  onToggle: () => void
}

const SIZE = 160
const COMPACT_SIZE = 124

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

export const ConnectButton = ({
  state,
  uptime,
  errorText,
  disabled,
  compact,
  onToggle,
}: Props) => {
  const { t } = useTranslation()
  const size = compact ? COMPACT_SIZE : SIZE

  const palette = {
    off: 'text.disabled',
    connecting: 'info.main',
    disconnecting: 'info.main',
    on: 'success.main',
    error: 'error.main',
  } as const
  const color = palette[state]

  const bgKey = {
    off: 'primary',
    connecting: 'info',
    disconnecting: 'info',
    on: 'success',
    error: 'error',
  } as const

  const label = {
    off: t('home.components.connect.states.off'),
    connecting: t('home.components.connect.states.connecting'),
    disconnecting: t('home.components.connect.states.disconnecting'),
    on: t('home.components.connect.states.on'),
    error: t('home.components.connect.states.error'),
  }[state]

  return (
    <Box
      data-connect-anchor=""
      sx={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 1,
      }}
    >
      <Box
        component="button"
        type="button"
        aria-label={label}
        aria-pressed={state === 'on'}
        disabled={disabled}
        onClick={onToggle}
        sx={(theme) => ({
          width: size,
          height: size,
          borderRadius: '50%',
          border: `2px solid ${theme.palette.divider}`,
          borderColor: color,
          background:
            state === 'on'
              ? `linear-gradient(160deg, ${alpha(
                  theme.palette.success.main,
                  0.22,
                )}, ${alpha(theme.palette.success.main, 0.1)})`
              : alpha(theme.palette[bgKey[state]].main, 0.06),
          boxShadow:
            state === 'on'
              ? `0 0 0 10px ${alpha(
                  theme.palette.success.main,
                  0.1,
                )}, 0 14px 34px ${alpha(theme.palette.success.main, 0.28)}`
              : state === 'connecting' || state === 'disconnecting'
                ? `0 0 0 10px ${alpha(theme.palette.info.main, 0.08)}`
                : 'none',
          color,
          cursor: disabled ? 'not-allowed' : 'pointer',
          opacity: disabled ? 0.5 : 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          transition: theme.transitions.create(
            ['transform', 'opacity', 'border-color', 'background', 'box-shadow'],
            { duration: theme.transitions.duration.short },
          ),
          '&:hover': { transform: disabled ? 'none' : 'scale(1.03)' },
          '&:active': { transform: disabled ? 'none' : 'scale(0.97)' },
          '@keyframes clodPulse': {
            '0%': { transform: 'scale(1)', opacity: 1 },
            '50%': { transform: 'scale(1.04)', opacity: 0.75 },
            '100%': { transform: 'scale(1)', opacity: 1 },
          },
          animation:
            state === 'connecting' || state === 'disconnecting'
              ? 'clodPulse 1.4s ease-in-out infinite'
              : 'none',
          '@media (prefers-reduced-motion: reduce)': { animation: 'none' },
        })}
      >
        {state === 'connecting' || state === 'disconnecting' ? (
          <CircularProgress size={compact ? 44 : 56} color="inherit" />
        ) : (
          <PowerSettingsNewRoundedIcon sx={{ fontSize: compact ? 50 : 64 }} />
        )}
      </Box>

      <Typography variant="subtitle1" sx={{ color, fontWeight: 600 }}>
        {label}
      </Typography>

      <Typography
        variant="body1"
        sx={{
          fontVariantNumeric: 'tabular-nums',
          letterSpacing: 1,
          fontWeight: 600,
          minHeight: 24,
          visibility:
            state === 'on' && uptime !== undefined ? 'visible' : 'hidden',
        }}
      >
        {state === 'on' && uptime !== undefined
          ? formatUptime(uptime)
          : '00:00'}
      </Typography>

      {state === 'error' && errorText ? (
        <Typography
          variant="body2"
          color="error"
          sx={{ maxWidth: 320, textAlign: 'center' }}
        >
          {errorText}
        </Typography>
      ) : null}
    </Box>
  )
}
