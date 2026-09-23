import { Box, Button, Snackbar, useTheme } from '@mui/material'
import { useLockFn } from 'ahooks'
import {
  useCallback,
  useImperativeHandle,
  useState,
  type ReactNode,
  type Ref,
} from 'react'
import { useTranslation } from 'react-i18next'
import { closeConnection } from 'tauri-plugin-mihomo-api'

import { showNotice } from '@/services/notice-service'
import parseTraffic from '@/utils/parse-traffic'

import { RelativeTime } from './connection-relative-time'

export interface ConnectionDetailRef {
  open: (id: string) => void
  close: () => void
}

interface Props {
  ref?: Ref<ConnectionDetailRef>
  activeConnections: IConnectionsItem[]
  closedConnections: IConnectionsItem[]
}

export function ConnectionDetail({
  ref,
  activeConnections,
  closedConnections,
}: Props) {
  const [detailId, setDetailId] = useState<string | null>(null)
  const theme = useTheme()

  const active = detailId
    ? activeConnections.find((item) => item.id === detailId)
    : undefined
  const detail =
    active ??
    (detailId
      ? closedConnections.find((item) => item.id === detailId)
      : undefined)
  const closed = active === undefined

  const onClose = useCallback(() => {
    setDetailId(null)
  }, [])

  useImperativeHandle(ref, () => ({
    open: (id: string) => {
      if (detail) return
      setDetailId(id)
    },
    close: onClose,
  }))

  return (
    <Snackbar
      anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
      open={detail !== undefined}
      onClose={onClose}
      sx={{
        '.MuiSnackbarContent-root': {
          maxWidth: '520px',
          maxHeight: '480px',
          overflowY: 'auto',
          backgroundColor: theme.palette.background.paper,
          color: theme.palette.text.primary,
        },
      }}
      message={
        detail ? (
          <InnerConnectionDetail
            data={detail}
            closed={closed}
            onClose={onClose}
          />
        ) : null
      }
    />
  )
}

interface InnerProps {
  data: IConnectionsItem
  closed: boolean
  onClose?: () => void
}

const InnerConnectionDetail = ({ data, closed, onClose }: InnerProps) => {
  const { t } = useTranslation()
  const { metadata, rulePayload } = data
  const theme = useTheme()
  const chains = [...data.chains].reverse().join(' / ')
  const rule = rulePayload ? `${data.rule}(${rulePayload})` : data.rule
  const hostAddress =
    metadata.host || metadata.destinationIP || metadata.remoteDestination
  const host = `${hostAddress}:${metadata.destinationPort}`
  const Destination = metadata.destinationIP
    ? metadata.destinationIP
    : metadata.remoteDestination

  const information: { label: string; value: ReactNode }[] = [
    { label: t('connections.components.fields.host'), value: host },
    {
      label: t('shared.labels.downloaded'),
      value: parseTraffic(data.download).join(' '),
    },
    {
      label: t('shared.labels.uploaded'),
      value: parseTraffic(data.upload).join(' '),
    },
    {
      label: t('connections.components.fields.dlSpeed'),
      value: parseTraffic(data.curDownload ?? -1).join(' ') + '/s',
    },
    {
      label: t('connections.components.fields.ulSpeed'),
      value: parseTraffic(data.curUpload ?? -1).join(' ') + '/s',
    },
    {
      label: t('connections.components.fields.chains'),
      value: chains,
    },
    { label: t('connections.components.fields.rule'), value: rule },
    {
      label: t('connections.components.fields.process'),
      value: `${metadata.process}${metadata.processPath ? `(${metadata.processPath})` : ''}`,
    },
    {
      label: t('connections.components.fields.time'),
      value: <RelativeTime start={data.start} />,
    },
    {
      label: t('connections.components.fields.source'),
      value: `${metadata.sourceIP}:${metadata.sourcePort}`,
    },
    {
      label: t('connections.components.fields.destination'),
      value: Destination,
    },
    {
      label: t('connections.components.fields.destinationPort'),
      value: `${metadata.destinationPort}`,
    },
    {
      label: t('connections.components.fields.type'),
      value: `${metadata.type}(${metadata.network})`,
    },
  ]

  const onDelete = useLockFn(async () => {
    try {
      await closeConnection(data.id)
      onClose?.()
    } catch (err) {
      showNotice.error(err)
    }
  })

  return (
    <Box sx={{ userSelect: 'text', color: theme.palette.text.secondary }}>
      {information.map((each) => (
        <div key={each.label}>
          <b>{each.label}</b>
          <span
            style={{
              wordBreak: 'break-all',
              color: theme.palette.text.primary,
            }}
          >
            : {each.value}
          </span>
        </div>
      ))}

      {!closed && (
        <Box sx={{ textAlign: 'right' }}>
          <Button
            variant="contained"
            title={t('connections.components.actions.closeConnection')}
            onClick={onDelete}
          >
            {t('connections.components.actions.closeConnection')}
          </Button>
        </Box>
      )}
    </Box>
  )
}
