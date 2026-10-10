import { ArrowForwardRounded, CloseRounded } from '@mui/icons-material'
import {
  alpha,
  Box,
  Button,
  IconButton,
  Snackbar,
  type Theme,
  useTheme,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import {
  Fragment,
  useCallback,
  useEffect,
  useImperativeHandle,
  useState,
  type ReactNode,
  type Ref,
} from 'react'
import { useTranslation } from 'react-i18next'
import { closeConnection } from 'tauri-plugin-mihomo-api'

import { showNotice } from '@/services/notice-service'
import parseTraffic, { parseSpeed } from '@/utils/parse-traffic'

import { HostText, RuleText, SpeedText } from './connection-parts'
import { RelativeTime } from './connection-relative-time'
import { getConnectionHostParts } from './connection-row-view'
import { connTextSx, outboundClass, outboundName } from './connection-text'

export interface ConnectionDetailRef {
  open: (id: string) => void
  close: () => void
}

interface Props {
  ref?: Ref<ConnectionDetailRef>
  activeConnections: IConnectionsItem[]
  closedConnections: IConnectionsItem[]
  onOpenChange?: (id: string | null) => void
}

export function ConnectionDetail({
  ref,
  activeConnections,
  closedConnections,
  onOpenChange,
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
  const openId = detail ? detail.id : null

  useEffect(() => {
    onOpenChange?.(openId)
  }, [onOpenChange, openId])

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
          width: 420,
          maxWidth: '100%',
          boxSizing: 'border-box',
          maxHeight: 'min(560px, calc(100vh - 96px))',
          overflowY: 'auto',
          alignItems: 'flex-start',
          px: 2,
          py: 1.25,
          borderRadius: '12px',
          backgroundColor: theme.palette.background.paper,
          backgroundImage: 'var(--Paper-overlay)',
          color: theme.palette.text.primary,
        },
        '.MuiSnackbarContent-message': { width: '100%', py: 0.5 },
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

const SectionTitle = ({ children }: { children: ReactNode }) => (
  <Box sx={{ mt: 1.75, mb: 0.75, fontSize: 12.5, fontWeight: 700 }}>
    {children}
  </Box>
)

const TrafficTile = ({
  label,
  total,
  speed,
  up,
}: {
  label: string
  total: number
  speed: number
  up?: boolean
}) => (
  <Box
    sx={({ palette }) => ({
      flex: 1,
      minWidth: 0,
      px: 1.5,
      py: 1,
      borderRadius: '8px',
      bgcolor: alpha(palette.text.primary, 0.045),
    })}
  >
    <Box
      sx={{
        fontSize: 11.5,
        fontWeight: 700,
        letterSpacing: '0.4px',
        textTransform: 'uppercase',
        color: 'text.secondary',
      }}
    >
      {label}
    </Box>
    <Box
      sx={{ fontSize: 17, fontWeight: 700, fontVariantNumeric: 'tabular-nums' }}
    >
      {parseTraffic(total)}
    </Box>
    <Box sx={{ fontSize: 12.5, fontVariantNumeric: 'tabular-nums' }}>
      <SpeedText value={speed} text={parseSpeed(speed)} up={up} arrow />
    </Box>
  </Box>
)

const stepSx = ({ palette }: Theme) => ({
  display: 'inline-flex',
  alignItems: 'center',
  minWidth: 0,
  maxWidth: '100%',
  px: 1,
  py: 0.5,
  borderRadius: '8px',
  fontSize: 13,
  overflowWrap: 'anywhere',
  bgcolor: alpha(palette.text.primary, 0.07),
  '&.cc-step-last': {
    fontWeight: 600,
    bgcolor: alpha(palette.primary.main, 0.14),
  },
})

const InnerConnectionDetail = ({ data, closed, onClose }: InnerProps) => {
  const { t } = useTranslation()
  const { metadata } = data
  const steps = [...data.chains].reverse()
  const directLabel = t('connections.components.summary.direct')
  const Destination = metadata.destinationIP
    ? metadata.destinationIP
    : metadata.remoteDestination

  const information: { label: string; value: ReactNode; mono?: boolean }[] = [
    {
      label: t('connections.components.fields.process'),
      value: metadata.process,
    },
    {
      label: t('connections.components.fields.processPath'),
      value: metadata.processPath,
      mono: true,
    },
    {
      label: t('connections.components.fields.source'),
      value: `${metadata.sourceIP}:${metadata.sourcePort}`,
      mono: true,
    },
    {
      label: t('connections.components.fields.destination'),
      value: Destination,
      mono: true,
    },
    {
      label: t('connections.components.fields.destinationPort'),
      value: metadata.destinationPort,
      mono: true,
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
    <Box sx={[connTextSx, { userSelect: 'text', color: 'text.primary' }]}>
      <Box sx={{ display: 'flex', alignItems: 'flex-start', gap: 1 }}>
        <Box sx={{ flex: 1, minWidth: 0 }}>
          <Box
            title={t('connections.components.fields.type')}
            sx={{ fontSize: 12, fontWeight: 600, color: 'text.secondary' }}
          >
            {metadata.network.toUpperCase()} · {metadata.type} ·{' '}
            <span title={t('connections.components.fields.time')}>
              <RelativeTime start={data.start} />
            </span>
          </Box>
          <Box
            className="cc-mono"
            title={t('connections.components.fields.host')}
            sx={{ fontSize: 15, fontWeight: 600, wordBreak: 'break-all' }}
          >
            <HostText {...getConnectionHostParts(metadata)} />
          </Box>
        </Box>
        <IconButton
          size="small"
          onClick={onClose}
          title={t('shared.actions.close')}
          aria-label={t('shared.actions.close')}
          sx={{ mt: -0.5, mr: -0.75 }}
        >
          <CloseRounded fontSize="small" />
        </IconButton>
      </Box>

      <SectionTitle>{t('connections.components.detail.traffic')}</SectionTitle>
      <Box sx={{ display: 'flex', gap: 1 }}>
        <TrafficTile
          label={t('shared.labels.downloaded')}
          total={data.download}
          speed={data.curDownload ?? 0}
        />
        <TrafficTile
          label={t('shared.labels.uploaded')}
          total={data.upload}
          speed={data.curUpload ?? 0}
          up
        />
      </Box>

      <SectionTitle>{t('connections.components.detail.route')}</SectionTitle>
      <Box
        sx={{
          display: 'flex',
          flexWrap: 'wrap',
          alignItems: 'center',
          gap: 0.75,
        }}
      >
        {steps.map((step, i) => (
          <Fragment key={step}>
            {i > 0 ? (
              <ArrowForwardRounded
                sx={{ fontSize: 14, color: 'text.disabled' }}
              />
            ) : null}
            <Box
              component="span"
              className={`${i === steps.length - 1 ? 'cc-step-last' : ''} ${outboundClass(step)}`}
              sx={stepSx}
            >
              {outboundName(step, directLabel)}
            </Box>
          </Fragment>
        ))}
      </Box>

      <SectionTitle>{t('connections.components.fields.rule')}</SectionTitle>
      <RuleText
        rule={data.rule}
        payload={data.rulePayload}
        fallback={t('connections.components.rule.fallback')}
        wrap
      />

      <SectionTitle>{t('connections.components.detail.details')}</SectionTitle>
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'minmax(110px, auto) minmax(0, 1fr)',
          gap: '6px 12px',
          fontSize: 13,
        }}
      >
        {information.map((each) => (
          <Fragment key={each.label}>
            <Box sx={{ color: 'text.secondary' }}>{each.label}</Box>
            <Box
              className={each.mono ? 'cc-mono' : undefined}
              sx={{ wordBreak: 'break-all' }}
            >
              {each.value || '—'}
            </Box>
          </Fragment>
        ))}
      </Box>

      {!closed && (
        <Box sx={{ mt: 2, textAlign: 'right' }}>
          <Button
            variant="outlined"
            color="error"
            size="small"
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
