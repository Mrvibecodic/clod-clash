import PowerSettingsNewRoundedIcon from '@mui/icons-material/PowerSettingsNewRounded'
import { Box, CircularProgress, Typography, alpha } from '@mui/material'
import { useTranslation } from 'react-i18next'

export type ConnectState = 'off' | 'connecting' | 'on' | 'error'

interface Props {
  state: ConnectState
  /** Seconds since the connection came up; only shown while `state` is `on`. */
  uptime?: number
  errorText?: string
  disabled?: boolean
  onToggle: () => void
}

const SIZE = 160

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

/**
 * The one button of the simple interface.
 *
 * The state comes from what the system actually reports, never from an
 * optimistic local flag — if the system proxy is dropped from outside the app,
 * the button has to go dark on its own.
 */
export const ConnectButton = ({
  state,
  uptime,
  errorText,
  disabled,
  onToggle,
}: Props) => {
  const { t } = useTranslation()

  const palette = {
    off: 'text.disabled',
    connecting: 'info.main',
    on: 'success.main',
    error: 'error.main',
  } as const
  const color = palette[state]

  const label = {
    off: t('home.components.connect.states.off'),
    connecting: t('home.components.connect.states.connecting'),
    on: t('home.components.connect.states.on'),
    error: t('home.components.connect.states.error'),
  }[state]

  return (
    <Box
      sx={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        gap: 1.5,
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
          width: SIZE,
          height: SIZE,
          borderRadius: '50%',
          border: `2px solid ${theme.palette.divider}`,
          borderColor: color,
          background: alpha(
            theme.palette[state === 'off' ? 'primary' : 'success'].main,
            state === 'on' ? 0.16 : 0.06,
          ),
          color,
          cursor: disabled ? 'not-allowed' : 'pointer',
          opacity: disabled ? 0.5 : 1,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          // Only transform and opacity are animated, so the button stays at
          // 60 fps even on the software renderer some Linux setups end up with.
          transition: theme.transitions.create(
            ['transform', 'opacity', 'border-color', 'background'],
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
            state === 'connecting'
              ? 'clodPulse 1.4s ease-in-out infinite'
              : 'none',
          '@media (prefers-reduced-motion: reduce)': { animation: 'none' },
        })}
      >
        {state === 'connecting' ? (
          <CircularProgress size={56} color="inherit" />
        ) : (
          <PowerSettingsNewRoundedIcon sx={{ fontSize: 64 }} />
        )}
      </Box>

      <Typography variant="subtitle1" sx={{ color, fontWeight: 600 }}>
        {label}
      </Typography>

      {state === 'on' && uptime !== undefined ? (
        <Typography
          variant="h6"
          sx={{ fontVariantNumeric: 'tabular-nums', letterSpacing: 1 }}
        >
          {formatUptime(uptime)}
        </Typography>
      ) : null}

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
