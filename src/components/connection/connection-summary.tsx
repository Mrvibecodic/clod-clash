import { Box, Typography } from '@mui/material'
import { type ReactNode, memo, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { CARD_SURFACE, CARD_TITLE, CARD_VALUE, NARROW } from '@/pages/_theme'
import parseTraffic, { parseSpeed } from '@/utils/parse-traffic'

import {
  type ConnectionSummaryEntry,
  summarizeConnections,
} from './connection-stats'

// clod:design-v3 — полоса итогов над таблицей: сама таблица отвечает на вопрос
// «что за соединение», а сводка — на «кто и через что съел трафик».
const SUMMARY_HEIGHT = 126

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
      sx={{
        ...CARD_TITLE,
        fontSize: 11.5,
        fontWeight: 700,
        letterSpacing: '0.4px',
        textTransform: 'uppercase',
        mb: 0.5,
        flexShrink: 0,
      }}
    >
      {title}
    </Typography>
    {children}
  </Box>
)

const SummaryBars = ({
  entries,
  accent,
}: {
  entries: ConnectionSummaryEntry[]
  accent?: string
}) => {
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
            sx={{
              fontSize: 12,
              flex: '0 0 38%',
              minWidth: 0,
              ...(entry.label === accent && {
                color: 'success.main',
                fontWeight: 600,
              }),
            }}
          >
            {entry.label}
          </Typography>
          <Box
            sx={{
              flex: 1,
              height: 4,
              minWidth: 0,
              borderRadius: 2,
              bgcolor: 'action.hover',
              overflow: 'hidden',
            }}
          >
            <Box
              sx={{
                height: '100%',
                borderRadius: 2,
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
            {parseTraffic(entry.value)}
          </Typography>
        </Box>
      ))}
    </Box>
  )
}

interface Props {
  connections: IConnectionsItem[]
  closed: boolean
}

export const ConnectionSummary = memo(function ConnectionSummary({
  connections,
  closed,
}: Props) {
  const { t } = useTranslation()

  const stats = useMemo(
    () =>
      summarizeConnections(connections, {
        noProcess: t('connections.components.summary.noProcess'),
        direct: t('connections.components.summary.direct'),
        unknownRoute: t('connections.components.summary.unknownRoute'),
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
        [NARROW]: {
          display: 'grid',
          gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
          height: 'auto',
          '& > :last-child:nth-child(odd)': { gridColumn: '1 / -1' },
        },
      }}
    >
      {!closed && (
        <SummaryCard title={t('connections.components.summary.now')}>
          <Typography noWrap sx={{ ...CARD_VALUE, color: 'primary.main' }}>
            ↓ {parseSpeed(stats.downloadSpeed)}
          </Typography>
          <Typography noWrap sx={{ fontSize: 12.5, color: 'secondary.main' }}>
            ↑ {parseSpeed(stats.uploadSpeed)}
          </Typography>
        </SummaryCard>
      )}
      <SummaryCard title={t('connections.components.summary.volume')}>
        <Typography noWrap sx={CARD_VALUE}>
          {parseTraffic(stats.download + stats.upload)}
        </Typography>
        <Typography noWrap sx={{ fontSize: 12.5, color: 'text.secondary' }}>
          ↓ {parseTraffic(stats.download)} · ↑ {parseTraffic(stats.upload)}
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
        <SummaryBars
          entries={stats.routes}
          accent={t('connections.components.summary.direct')}
        />
      </SummaryCard>
    </Box>
  )
})
