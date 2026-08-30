import { Box, Typography } from '@mui/material'
import { type ReactNode, memo, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { CARD_SURFACE, CARD_TITLE, CARD_VALUE } from '@/pages/_theme'
import parseTraffic from '@/utils/parse-traffic'

import {
  type ConnectionSummaryEntry,
  summarizeConnections,
} from './connection-stats'

// clod:design-v3 — полоса итогов над таблицей: сама таблица отвечает на вопрос
// «что за соединение», а сводка — на «кто и через что съел трафик».
const SUMMARY_HEIGHT = 126

const formatTotal = (bytes: number) => parseTraffic(bytes).join(' ')
const formatSpeed = (bytes: number) => `${parseTraffic(bytes).join(' ')}/s`

const SummaryCard = ({
  title,
  children,
}: {
  title: string
  children: ReactNode
}) => (
  <Box
    sx={{
      ...CARD_SURFACE,
      flex: 1,
      minWidth: 0,
      px: 1.5,
      py: 1,
      display: 'flex',
      flexDirection: 'column',
      overflow: 'hidden',
    }}
  >
    <Typography
      variant="caption"
      color="text.secondary"
      noWrap
      sx={{ ...CARD_TITLE, mb: 0.5, flexShrink: 0 }}
    >
      {title}
    </Typography>
    {children}
  </Box>
)

const SummaryBars = ({ entries }: { entries: ConnectionSummaryEntry[] }) => {
  const max = entries.length > 0 ? entries[0].value : 0

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 0.5 }}>
      {entries.map((entry) => (
        <Box
          key={entry.key}
          sx={{ display: 'flex', alignItems: 'center', gap: 1, minWidth: 0 }}
        >
          <Typography
            noWrap
            title={entry.label}
            sx={{ fontSize: 12, flex: '0 0 38%', minWidth: 0 }}
          >
            {entry.label}
          </Typography>
          <Box
            sx={{
              flex: 1,
              height: 6,
              minWidth: 0,
              borderRadius: 3,
              bgcolor: 'action.hover',
              overflow: 'hidden',
            }}
          >
            <Box
              sx={{
                height: '100%',
                borderRadius: 3,
                bgcolor: 'primary.main',
                width: `${max > 0 ? Math.max(2, (entry.value / max) * 100) : 0}%`,
              }}
            />
          </Box>
          <Typography
            noWrap
            sx={{
              fontSize: 11.5,
              color: 'text.secondary',
              flex: '0 0 64px',
              textAlign: 'right',
              fontVariantNumeric: 'tabular-nums',
            }}
          >
            {formatTotal(entry.value)}
          </Typography>
        </Box>
      ))}
    </Box>
  )
}

interface Props {
  connections: IConnectionsItem[]
}

export const ConnectionSummary = memo(function ConnectionSummary({
  connections,
}: Props) {
  const { t } = useTranslation()

  const stats = useMemo(
    () =>
      summarizeConnections(connections, {
        noProcess: t('connections.components.summary.noProcess'),
        direct: t('connections.components.summary.direct'),
      }),
    [connections, t],
  )

  return (
    <Box
      sx={{
        display: 'flex',
        gap: 1,
        mx: '10px',
        mt: 1,
        height: SUMMARY_HEIGHT,
        flex: '0 0 auto',
      }}
    >
      <SummaryCard title={t('connections.components.summary.now')}>
        <Typography noWrap sx={{ ...CARD_VALUE, color: 'primary.main' }}>
          ↓ {formatSpeed(stats.downloadSpeed)}
        </Typography>
        <Typography noWrap sx={{ fontSize: 12.5, color: 'secondary.main' }}>
          ↑ {formatSpeed(stats.uploadSpeed)}
        </Typography>
      </SummaryCard>
      <SummaryCard title={t('connections.components.summary.volume')}>
        <Typography noWrap sx={CARD_VALUE}>
          {formatTotal(stats.download + stats.upload)}
        </Typography>
        <Typography noWrap sx={{ fontSize: 12.5, color: 'text.secondary' }}>
          ↓ {formatTotal(stats.download)} · ↑ {formatTotal(stats.upload)}
        </Typography>
        <Typography noWrap sx={{ fontSize: 12.5, color: 'text.secondary' }}>
          {t('connections.components.summary.shown')}: {connections.length} ·{' '}
          {t('connections.components.summary.processes')}: {stats.processCount}
        </Typography>
      </SummaryCard>
      <SummaryCard title={t('connections.components.summary.processes')}>
        <SummaryBars entries={stats.processes} />
      </SummaryCard>
      <SummaryCard title={t('connections.components.summary.routes')}>
        <SummaryBars entries={stats.routes} />
      </SummaryCard>
    </Box>
  )
})
