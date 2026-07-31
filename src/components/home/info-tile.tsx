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
    <Stack sx={{ gap: 0.75, position: 'relative', minWidth: 0, pr: 7 }}>
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
