import NetworkCheckRoundedIcon from '@mui/icons-material/NetworkCheckRounded'
import { Box, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { InfoTile } from '@/components/home/info-tile'
import { useTrafficData } from '@/hooks/use-traffic-data'
import { useVisibility } from '@/hooks/use-visibility'
import { CARD_VALUE } from '@/pages/_theme'
import parseTraffic from '@/utils/parse-traffic'

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
  const pageVisible = useVisibility()

  const {
    response: { data: traffic },
  } = useTrafficData({ enabled: pageVisible })

  const speed = (bytes: number) => `${parseTraffic(bytes).join(' ')}/s`
  const total = (bytes: number) => parseTraffic(bytes).join(' ')

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
    </InfoTile>
  )
}
