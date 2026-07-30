import { Box, Stack, Typography } from '@mui/material'
import { useRef } from 'react'
import { useTranslation } from 'react-i18next'

import {
  EnhancedCanvasTrafficGraph,
  type EnhancedCanvasTrafficGraphRef,
} from '@/components/home/enhanced-canvas-traffic-graph'
import { useTrafficData } from '@/hooks/use-traffic-data'
import { useVisibility } from '@/hooks/use-visibility'
import parseTraffic from '@/utils/parse-traffic'

/** One legend entry: a coloured dot and the current speed. */
const Legend = ({ color, value }: { color: string; value: string }) => (
  <Stack
    direction="row"
    sx={{
      alignItems: 'center',
      gap: 0.75,
      fontVariantNumeric: 'tabular-nums',
    }}
  >
    <Box
      sx={{
        width: 8,
        height: 8,
        borderRadius: '50%',
        bgcolor: color,
        flex: 'none',
      }}
    />
    <Typography sx={{ fontSize: 12.5 }} color="text.secondary">
      {value}
    </Typography>
  </Stack>
)

/**
 * The mockups' «Network» card: a title row with live up/down legends and a
 * compact traffic sparkline — instead of the upstream six-tile stats block.
 */
export const NetCard = () => {
  const { t } = useTranslation()
  const trafficRef = useRef<EnhancedCanvasTrafficGraphRef>(null)
  const pageVisible = useVisibility()

  const {
    response: { data: traffic },
  } = useTrafficData({ enabled: pageVisible })

  const speed = (bytes: number) => `${parseTraffic(bytes).join(' ')}/s`

  return (
    <Stack
      sx={{
        gap: 1,
        p: 1.75,
        borderRadius: '14px',
        bgcolor: 'background.paper',
        border: (theme) => `1px solid ${theme.palette.divider}`,
      }}
    >
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.75 }}>
        <Typography sx={{ fontSize: 13, fontWeight: 600, flex: 1 }}>
          {t('home.components.net.title')}
        </Typography>
        <Legend color="#2E7CF6" value={`↓ ${speed(traffic?.down ?? 0)}`} />
        <Legend color="#EA580C" value={`↑ ${speed(traffic?.up ?? 0)}`} />
      </Stack>
      <Box
        sx={{ height: 72, cursor: 'pointer' }}
        onClick={() => trafficRef.current?.toggleStyle()}
      >
        <EnhancedCanvasTrafficGraph ref={trafficRef} minimal />
      </Box>
    </Stack>
  )
}
