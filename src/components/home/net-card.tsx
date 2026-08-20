import NetworkCheckRoundedIcon from '@mui/icons-material/NetworkCheckRounded'
import { Box, Typography, useTheme } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { InfoTile } from '@/components/home/info-tile'
import { useTrafficData } from '@/hooks/use-traffic-data'
import { useTrafficMonitorEnhanced } from '@/hooks/use-traffic-monitor'
import { useVisibility } from '@/hooks/use-visibility'
import { CARD_VALUE } from '@/pages/_theme'
import parseTraffic from '@/utils/parse-traffic'

const SPARK_POINTS = 60
const SPARK_WIDTH = 300
const SPARK_HEIGHT = 36

const buildPoints = (values: number[], max: number) => {
  const step = SPARK_WIDTH / (values.length - 1)
  return values
    .map((value, index) => {
      const x = (index * step).toFixed(1)
      const y = (
        SPARK_HEIGHT -
        2 -
        (value / max) * (SPARK_HEIGHT - 4)
      ).toFixed(1)
      return `${x},${y}`
    })
    .join(' ')
}

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
    <Typography noWrap sx={{ ...CARD_VALUE, color: color ?? 'text.primary' }}>
      {value}
    </Typography>
  </Box>
)

export const NetCard = () => {
  const { t } = useTranslation()
  const theme = useTheme()
  const pageVisible = useVisibility()

  const {
    response: { data: traffic },
  } = useTrafficData({ enabled: pageVisible })
  const {
    graphData: { dataPoints },
  } = useTrafficMonitorEnhanced({ enabled: pageVisible })

  const speed = (bytes: number) => `${parseTraffic(bytes).join(' ')}/s`
  const total = (bytes: number) => parseTraffic(bytes).join(' ')

  const recent = dataPoints.slice(-SPARK_POINTS)
  const down = recent.map((point) => point.down)
  const up = recent.map((point) => point.up)
  const max = Math.max(1, ...down, ...up)

  return (
    <InfoTile
      title={t('home.components.net.title')}
      icon={<NetworkCheckRoundedIcon />}
    >
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
          color="primary.main"
        />
        <Stat
          label={`↑ ${t('home.components.net.upSpeed')}`}
          value={speed(traffic?.up ?? 0)}
          color="secondary.main"
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
      {recent.length >= 2 ? (
        <Box
          component="svg"
          viewBox={`0 0 ${SPARK_WIDTH} ${SPARK_HEIGHT}`}
          preserveAspectRatio="none"
          aria-hidden
          sx={{ display: 'block', width: '100%', height: SPARK_HEIGHT, mt: 1 }}
        >
          <polyline
            fill="none"
            stroke={theme.palette.primary.main}
            strokeWidth={2}
            strokeLinejoin="round"
            strokeLinecap="round"
            points={buildPoints(down, max)}
          />
          <polyline
            fill="none"
            stroke={theme.palette.secondary.main}
            strokeWidth={1.6}
            strokeLinejoin="round"
            strokeLinecap="round"
            opacity={0.65}
            points={buildPoints(up, max)}
          />
        </Box>
      ) : null}
    </InfoTile>
  )
}
