import { CloseRounded } from '@mui/icons-material'
import { Box, IconButton, type Theme } from '@mui/material'
import { useLockFn } from 'ahooks'
import { memo, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { closeConnection } from 'tauri-plugin-mihomo-api'

import { RelativeTime } from './connection-relative-time'
import type { ConnectionRowView } from './connection-row-view'

interface Props {
  row: ConnectionRowView
  closed: boolean
  onShowDetail: (id: string) => void
}

const tagStyle = {
  boxSizing: 'border-box',
  maxWidth: '100%',
  padding: '0 5px',
  // clod:design-v3 — та же линия, что у карточек и разделителей, вместо
  // собственного серого 0.35.
  border: '1px solid var(--divider-color)',
  borderRadius: 8,
  fontSize: 10,
  lineHeight: 1.375,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
} as const

// clod:design-v3 — строка переехала с инлайнового style на sx: инлайн не
// умеет :hover, поэтому список соединений был единственным, который никак не
// отзывался на курсор.
const itemSx = {
  boxSizing: 'border-box',
  minHeight: 56,
  display: 'flex',
  alignItems: 'center',
  gap: 1,
  padding: '6px 48px 6px 12px',
  borderBottom: '1px solid var(--divider-color)',
  position: 'relative',
  overflow: 'hidden',
  transition: (theme: Theme) =>
    theme.transitions.create(['background-color'], {
      duration: theme.transitions.duration.short,
    }),
  '&:hover': { bgcolor: 'action.hover' },
} as const

const contentStyle = {
  minWidth: 0,
  flex: 1,
  cursor: 'pointer',
  userSelect: 'text',
} as const

const primaryStyle = {
  fontSize: 14,
  lineHeight: 1.4,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
} as const

const tagsStyle = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 4,
  marginTop: 4,
  overflow: 'hidden',
} as const

const actionStyle = {
  position: 'absolute',
  right: 8,
  top: '50%',
  transform: 'translateY(-50%)',
} as const

export const ConnectionRowItem = memo(
  function ConnectionRowItem({ row, closed, onShowDetail }: Props) {
    const { t } = useTranslation()
    const onDelete = useLockFn(async () => closeConnection(row.id))
    const handleShowDetail = useCallback(
      () => onShowDetail(row.id),
      [onShowDetail, row.id],
    )
    const showTraffic = row.uploadSpeed >= 100 || row.downloadSpeed >= 100

    return (
      <Box sx={itemSx}>
        <div style={contentStyle} onClick={handleShowDetail}>
          <div style={primaryStyle}>{row.host}</div>
          <div style={tagsStyle}>
            <span style={tagStyle}>{row.network}</span>
            <span style={tagStyle}>{row.type}</span>
            {row.process && <span style={tagStyle}>{row.process}</span>}
            {row.chains && <span style={tagStyle}>{row.chains}</span>}
            <span style={tagStyle}>
              <RelativeTime start={row.time} />
            </span>
            {showTraffic && (
              <span style={tagStyle}>
                {row.uploadSpeedText} / {row.downloadSpeedText}
              </span>
            )}
          </div>
        </div>
        {!closed && (
          <IconButton
            size="small"
            color="inherit"
            onClick={onDelete}
            title={t('connections.components.actions.closeConnection')}
            aria-label={t('connections.components.actions.closeConnection')}
            sx={actionStyle}
          >
            <CloseRounded fontSize="small" />
          </IconButton>
        )}
      </Box>
    )
  },
  (prev, next) =>
    prev.row === next.row &&
    prev.closed === next.closed &&
    prev.onShowDetail === next.onShowDetail,
)
