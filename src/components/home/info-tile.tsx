import { Box, Stack, Typography } from '@mui/material'
import type { ReactNode } from 'react'

import { CARD_LIFT, CARD_SURFACE, CARD_TITLE } from '@/pages/_theme'

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
      ...CARD_SURFACE,
      ...CARD_LIFT,
      position: 'relative',
      overflow: 'hidden',
      minWidth: 0,
      p: 1.75,
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
        opacity: 0.07,
        pointerEvents: 'none',
        color: 'primary.main',
        '& svg': { fontSize: 72 },
      }}
    >
      {icon}
    </Box>
    <Stack sx={{ gap: 0.75, position: 'relative', minWidth: 0, pr: '86px' }}>
      <Typography
        variant="caption"
        color="text.secondary"
        sx={CARD_TITLE}
        noWrap
      >
        {title}
      </Typography>
      {children}
    </Stack>
  </Box>
)
