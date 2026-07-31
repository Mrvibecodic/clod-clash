import { Box, Stack, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { useTrafficData } from '@/hooks/use-traffic-data'
import { useVisibility } from '@/hooks/use-visibility'
import parseTraffic from '@/utils/parse-traffic'

/** One stat cell: a small label over a bold tabular value. */
const Stat = ({
  label,
  value,
  color,
}: {
  label: string
  value: string
  color?: string
}) => (
  <Box sx={{ minWidth: 0 }}>
    <Typography
      variant="caption"
      color="text.secondary"
      noWrap
      sx={{ display: 'block' }}
    >
      {label}
    </Typography>
    <Typography
      noWrap
      sx={{
        fontSize: 15,
        fontWeight: 700,
        fontVariantNumeric: 'tabular-nums',
        color: color ?? 'text.primary',
      }}
    >
      {value}
    </Typography>
  </Box>
)

/**
 * The «Network» card: four numbers instead of a graph — the momentary
 * speeds and the session totals. A sparkline looked technical and told a
 * subscription user nothing actionable (removed by request, 31.07).
 */
export const NetCard = () => {
  const { t } = useTranslation()
  const pageVisible = useVisibility()

  const {
    response: { data: traffic },
  } = useTrafficData({ enabled: pageVisible })

  const speed = (bytes: number) => `${parseTraffic(bytes).join(' ')}/s`
  const total = (bytes: number) => parseTraffic(bytes).join(' ')

  return (
    <Stack
      sx={{
        gap: 1.25,
        p: 1.75,
        borderRadius: '14px',
        bgcolor: 'background.paper',
        border: (theme) => `1px solid ${theme.palette.divider}`,
      }}
    >
      <Typography sx={{ fontSize: 13, fontWeight: 600 }}>
        {t('home.components.net.title')}
      </Typography>
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
          rowGap: 1.25,
          columnGap: 1,
        }}
      >
        <Stat
          label={`↓ ${t('home.components.net.downSpeed')}`}
          value={speed(traffic?.down ?? 0)}
          color="#2E7CF6"
        />
        <Stat
          label={`↑ ${t('home.components.net.upSpeed')}`}
          value={speed(traffic?.up ?? 0)}
          color="#EA580C"
        />
        <Stat
          label={t('home.components.net.downloaded')}
          value={total(traffic?.downTotal ?? 0)}
        />
        <Stat
          label={t('home.components.net.uploaded')}
          value={total(traffic?.upTotal ?? 0)}
        />
      </Box>
    </Stack>
  )
}
