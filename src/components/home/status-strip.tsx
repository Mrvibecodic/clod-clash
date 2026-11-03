import {
  alpha,
  Button,
  CircularProgress,
  Stack,
  Typography,
} from '@mui/material'
import type { ReactNode } from 'react'

import { TINT } from '@/pages/_theme'

interface StatusStripProps {
  tone: 'error' | 'warning'
  icon: ReactNode
  text: ReactNode
  action?: { label: ReactNode; onClick: () => void; disabled?: boolean }
}

export const StatusStrip = ({ tone, icon, text, action }: StatusStripProps) => (
  <Stack
    direction="row"
    sx={{
      maxWidth: '100%',
      alignItems: 'center',
      gap: 1,
      px: 1.25,
      py: 0.75,
      borderRadius: '12px',
      bgcolor: (theme) => alpha(theme.palette[tone].main, TINT.base),
      border: (theme) =>
        `1px solid ${alpha(theme.palette[tone].main, TINT.edge)}`,
    }}
  >
    {icon}
    <Typography
      variant="caption"
      color="text.primary"
      sx={{ flex: 1, minWidth: 0 }}
    >
      {text}
    </Typography>
    {action && (
      <Button
        size="small"
        sx={{ flex: 'none' }}
        disabled={action.disabled}
        onClick={action.onClick}
      >
        {action.label}
      </Button>
    )}
  </Stack>
)

export const StatusBusyLine = ({ text }: { text: ReactNode }) => (
  <Stack
    direction="row"
    sx={{ alignItems: 'center', justifyContent: 'center', gap: 1, py: 0.5 }}
  >
    <CircularProgress size={14} />
    <Typography variant="caption" color="text.secondary">
      {text}
    </Typography>
  </Stack>
)
