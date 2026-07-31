import { Box, Stack, Typography } from '@mui/material'
import type { ReactNode } from 'react'

/**
 * A home-screen info tile: small title, content, and a big faded icon as the
 * tile's «meaning» (traffic, calendar, network…). The icon sits at the right
 * edge, vertically centred and fully inside the tile — not bleeding out of a
 * corner (user feedback, 31.07).
 */
export const InfoTile = ({
  title,
  icon,
  children,
}: {
  title: string
  icon: ReactNode
  children: ReactNode
}) => (
  <Box
    sx={{
      position: 'relative',
      overflow: 'hidden',
      minWidth: 0,
      p: 1.75,
      borderRadius: '14px',
      bgcolor: 'background.paper',
      border: (theme) => `1px solid ${theme.palette.divider}`,
    }}
  >
    <Box
      aria-hidden
      sx={{
        position: 'absolute',
        right: 14,
        top: '50%',
        transform: 'translateY(-50%)',
        display: 'flex',
        alignItems: 'center',
        opacity: 0.09,
        pointerEvents: 'none',
        color: 'primary.main',
        '& svg': { fontSize: 72 },
      }}
    >
      {icon}
    </Box>
    {/* clod: контент (включая полоску трафика) заканчивается за 14px до
        иконки — тот же отступ, что и слева от края плитки. Иконка: 72px
        шириной, right:14; padding плитки 14 → 72 + 14 + 14 − 14 = 86px */}
    <Stack sx={{ gap: 0.75, position: 'relative', minWidth: 0, pr: '86px' }}>
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ fontWeight: 600, letterSpacing: 0.2 }}
        noWrap
      >
        {title}
      </Typography>
      {children}
    </Stack>
  </Box>
)
