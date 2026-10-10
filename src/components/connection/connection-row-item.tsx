import { CloseRounded } from '@mui/icons-material'
import { alpha, Box, IconButton, type Theme } from '@mui/material'
import { useLockFn } from 'ahooks'
import { memo, type MouseEvent, useCallback } from 'react'
import { useTranslation } from 'react-i18next'
import { closeConnection } from 'tauri-plugin-mihomo-api'

import { showNotice } from '@/services/notice-service'

import { ChainText, HostText, SpeedText } from './connection-parts'
import { RelativeTime } from './connection-relative-time'
import type { ConnectionRowView } from './connection-row-view'
import { connTileSx } from './connection-text'

interface Props {
  row: ConnectionRowView
  closed: boolean
  selected?: boolean
  onShowDetail: (id: string) => void
}

// clod:design-v3 — строка на sx, а не на инлайновом style: инлайн не умеет
// :hover, и список соединений никак не отзывался на курсор.
const itemSx = [
  ({ palette }: Theme) => ({
    display: 'grid',
    gridTemplateColumns: 'minmax(0, 1fr) auto 34px',
    alignItems: 'center',
    gap: 1.25,
    minHeight: 52,
    boxSizing: 'border-box',
    py: 0.75,
    pl: 1.5,
    pr: 1,
    borderRadius: '10px',
    '& .cc-mchip': {
      flex: 'none',
      px: 0.625,
      borderRadius: '5px',
      bgcolor: alpha(palette.text.primary, 0.07),
      color: 'text.secondary',
      fontFamily: 'monospace',
      fontSize: 11,
      lineHeight: 1.6,
    },
  }),
  connTileSx,
] as const

const contentStyle = {
  minWidth: 0,
  userSelect: 'text',
} as const

const hostStyle = {
  fontSize: 13.5,
  lineHeight: 1.4,
  overflow: 'hidden',
  textOverflow: 'ellipsis',
  whiteSpace: 'nowrap',
} as const

const metaStyle = {
  marginTop: 3,
  fontSize: 12.5,
} as const

const metaGroupStyle = {
  display: 'flex',
  alignItems: 'center',
  gap: 6,
  minWidth: 0,
  maxWidth: '100%',
  whiteSpace: 'nowrap',
  overflow: 'hidden',
} as const

const processStyle = {
  flex: '0 1 auto',
  maxWidth: 180,
} as const

const speedStyle = {
  display: 'flex',
  flexDirection: 'column',
  alignItems: 'flex-end',
  gap: 2,
  fontSize: 12.5,
  fontVariantNumeric: 'tabular-nums',
  whiteSpace: 'nowrap',
} as const

export const ConnectionRowItem = memo(
  function ConnectionRowItem({ row, closed, selected, onShowDetail }: Props) {
    const { t } = useTranslation()
    const onDelete = useLockFn(async () => {
      try {
        await closeConnection(row.id)
      } catch (err) {
        showNotice.error(err)
      }
    })
    const handleDelete = useCallback(
      (event: MouseEvent) => {
        event.stopPropagation()
        onDelete()
      },
      [onDelete],
    )
    const handleShowDetail = useCallback(
      () => onShowDetail(row.id),
      [onShowDetail, row.id],
    )
    return (
      <div style={{ padding: '3px 0' }}>
        <Box
          sx={itemSx}
          data-selected={Boolean(selected)}
          onClick={handleShowDetail}
        >
          <div style={contentStyle}>
            <div className="cc-mono" style={hostStyle}>
              <HostText host={row.host} port={row.port} />
            </div>
            <div className="cc-sec cc-meta" style={metaStyle}>
              <span className="cc-mg" style={metaGroupStyle}>
                <span className="cc-mchip">{row.network}</span>
                <span className="cc-mchip">{row.type}</span>
                {row.process && (
                  <span
                    className="cc-cut"
                    style={processStyle}
                    title={row.process}
                  >
                    {row.process}
                  </span>
                )}
              </span>
              {row.chainList.length > 0 && (
                <span className="cc-mg" style={metaGroupStyle}>
                  <ChainText
                    chains={row.chainList}
                    directLabel={t('connections.components.summary.direct')}
                  />
                </span>
              )}
              <span className="cc-mg" style={metaGroupStyle}>
                <RelativeTime start={row.time} />
              </span>
            </div>
          </div>
          <div style={speedStyle}>
            <SpeedText
              value={row.downloadSpeed}
              text={row.downloadSpeedText}
              arrow
            />
            <SpeedText
              value={row.uploadSpeed}
              text={row.uploadSpeedText}
              up
              arrow
            />
          </div>
          {!closed ? (
            <IconButton
              size="small"
              color="inherit"
              onClick={handleDelete}
              title={t('connections.components.actions.closeConnection')}
              aria-label={t('connections.components.actions.closeConnection')}
              sx={{ color: 'text.secondary' }}
            >
              <CloseRounded fontSize="small" />
            </IconButton>
          ) : (
            <span />
          )}
        </Box>
      </div>
    )
  },
  (prev, next) =>
    prev.row === next.row &&
    prev.closed === next.closed &&
    prev.selected === next.selected &&
    prev.onShowDetail === next.onShowDetail,
)
