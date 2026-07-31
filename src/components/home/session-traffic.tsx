import { Stack, Typography } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { useTrafficData } from '@/hooks/use-traffic-data'
import { useVisibility } from '@/hooks/use-visibility'
import parseTraffic from '@/utils/parse-traffic'

/**
 * Koala-style session counters: how much came down and went up since the
 * core started. One quiet line — it belongs near the Connect button, not
 * in a card of its own.
 */
export const SessionTraffic = () => {
  const { t } = useTranslation()
  const pageVisible = useVisibility()
  const {
    response: { data: traffic },
  } = useTrafficData({ enabled: pageVisible })

  const total = (bytes: number) => parseTraffic(bytes).join(' ')

  return (
    <Stack
      direction="row"
      sx={{
        gap: 0.75,
        justifyContent: 'center',
        alignItems: 'center',
        fontVariantNumeric: 'tabular-nums',
      }}
      title={t('home.components.net.session')}
    >
      <Typography variant="caption" color="text.secondary">
        ↓ {total(traffic?.downTotal ?? 0)}
      </Typography>
      <Typography variant="caption" color="text.disabled">
        ·
      </Typography>
      <Typography variant="caption" color="text.secondary">
        ↑ {total(traffic?.upTotal ?? 0)}
      </Typography>
    </Stack>
  )
}
