import { AddRounded, DeleteRounded } from '@mui/icons-material'
import {
  Box,
  Button,
  IconButton,
  MenuItem,
  Select,
  TextField,
  Typography,
} from '@mui/material'
import {
  forwardRef,
  type ReactNode,
  useImperativeHandle,
  useMemo,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  BaseSegmented,
  FormField,
  FormHint,
  FormRow,
  FormSection,
  FormTile,
  TypeChip,
} from '@/components/base'
import { MONO_INPUT, MONO_TEXT } from '@/components/base/base-mono'
import { useChangeCount } from '@/hooks/use-change-count'
import { useClash } from '@/hooks/use-clash'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import { isPortInUse } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import {
  formatHostPort,
  isValidPort,
  normalizeHost,
  normalizeListenHost,
} from '@/utils/network'

interface TunnelsViewerRef {
  open: () => void
  close: () => void
}

interface TunnelEntry {
  network: string[]
  address: string
  target: string
  proxy?: string
}

const NETWORK_OPTIONS = [
  { value: 'tcp', label: 'TCP' },
  { value: 'udp', label: 'UDP' },
  { value: 'tcp+udp', label: 'TCP + UDP' },
] as const

const FIELD_SX = {
  display: 'flex',
  flexDirection: 'column',
  justifyContent: 'flex-end',
} as const

const tunnelKey = (tunnel: TunnelEntry) =>
  `${tunnel.address}_${tunnel.target}_${tunnel.network.join('+')}`

const tunnelSet = (tunnels: TunnelEntry[]) => {
  const counts: Record<string, number> = {}
  const set: Record<string, string> = {}
  for (const tunnel of tunnels) {
    const base = tunnelKey(tunnel)
    counts[base] = (counts[base] ?? 0) + 1
    set[`${base}_${counts[base]}`] = tunnel.proxy ?? ''
  }
  return set
}

const Pending = ({ value, hint }: { value: string; hint: string }) =>
  value ? (
    <>{value}</>
  ) : (
    <Box component="span" sx={{ color: 'text.disabled' }}>
      {hint}
    </Box>
  )

export const TunnelsViewer = forwardRef<TunnelsViewerRef>((_, ref) => {
  const { t } = useTranslation()
  const { runtime, patchClash } = useClash()
  const { refreshProxy } = useAppRefreshers()

  const [open, setOpen] = useState(false)
  const [values, setValues] = useState({
    localAddr: '',
    localPort: '',
    targetAddr: '',
    targetPort: '',
    network: 'tcp+udp',
    group: '',
    proxy: '',
  })
  const [draftTunnels, setDraftTunnels] = useState<TunnelEntry[]>([])
  const [initialTunnels, setInitialTunnels] = useState<TunnelEntry[]>([])
  const changes = useChangeCount(
    useMemo(() => tunnelSet(initialTunnels), [initialTunnels]),
    useMemo(() => tunnelSet(draftTunnels), [draftTunnels]),
  )

  useImperativeHandle(ref, () => ({
    open: () => {
      setValues(() => ({
        localAddr: '',
        localPort: '',
        targetAddr: '',
        targetPort: '',
        network: 'tcp+udp',
        group: '',
        proxy: '',
      }))
      setDraftTunnels(() => runtime?.tunnels ?? [])
      setInitialTunnels(runtime?.tunnels ?? [])
      // Группы опрашиваются, только пока они на Главной или «Прокси», — здесь
      // их показываем свежими сами.
      refreshProxy().catch(() => {})
      setOpen(true)
    },
    close: () => {
      setOpen(false)
    },
  }))

  const tunnelEntries = useMemo(() => {
    const counts: Record<string, number> = {}
    return draftTunnels.map((tunnel, index) => {
      const base = tunnelKey(tunnel)
      const occurrence = (counts[base] = (counts[base] ?? 0) + 1)
      return {
        index,
        key: `${base}_${occurrence}`,
        address: tunnel.address,
        target: tunnel.target,
        network: tunnel.network,
        proxy: tunnel.proxy,
      }
    })
  }, [draftTunnels])

  const { proxies } = useProxiesData()

  const proxyGroups = useMemo<IProxyGroupItem[]>(() => {
    return proxies?.groups ?? []
  }, [proxies])

  const groupNames = useMemo<string[]>(
    () => proxyGroups.map((group) => group.name),
    [proxyGroups],
  )

  const proxyOptions = useMemo<IProxyItem[]>(() => {
    const group = proxyGroups.find((item) => item.name === values.group)
    return (group?.all ?? []).filter(
      (node) => !node.provider && node.type !== 'unknown',
    )
  }, [proxyGroups, values.group])

  const handleSave = async () => {
    try {
      await patchClash({ tunnels: draftTunnels })
      showNotice.success('shared.feedback.notifications.common.saveSuccess')
      setOpen(false)
    } catch (err: any) {
      showNotice.error(err)
    }
  }

  const handleAdd = async () => {
    const { localAddr, localPort, targetAddr, targetPort, network, proxy } =
      values

    // Базовая проверка на непустоту
    if (!localAddr || !localPort || !targetAddr || !targetPort) {
      showNotice.error(
        'settings.sections.clash.form.fields.tunnels.messages.incomplete',
      )
      return
    }

    // Проверка локального адреса (host)
    const localHost = normalizeListenHost(localAddr)
    if (!localHost) {
      showNotice.error(
        'settings.sections.clash.form.fields.tunnels.messages.invalidLocalAddr',
      )
      return
    }

    // Проверка локального порта (port)
    if (!isValidPort(localPort)) {
      showNotice.error(
        'settings.sections.clash.form.fields.tunnels.messages.invalidLocalPort',
      )
      return
    }
    const inUse = await isPortInUse(Number(localPort))
    if (inUse) {
      showNotice.error('settings.modals.clashPort.messages.portInUse', {
        port: localPort,
      })
      return
    }

    // Проверка целевого адреса (host)
    const targetHost = normalizeHost(targetAddr)
    if (!targetHost) {
      showNotice.error(
        'settings.sections.clash.form.fields.tunnels.messages.invalidTargetAddr',
      )
      return
    }

    // Проверка целевого порта (port)
    if (!isValidPort(targetPort)) {
      showNotice.error(
        'settings.sections.clash.form.fields.tunnels.messages.invalidTargetPort',
      )
      return
    }

    // Формируем новую entry
    const entry: TunnelEntry = {
      network: network === 'tcp+udp' ? ['tcp', 'udp'] : [network],
      address: formatHostPort(localHost, localPort),
      target: formatHostPort(targetHost, targetPort),
      ...(proxy ? { proxy } : {}),
    }

    // Записываем в конфиг + очищаем ввод
    setDraftTunnels((prev) => [...prev, entry])

    setValues((v) => ({
      ...v,
      localAddr: '',
      localPort: '',
      targetAddr: '',
      targetPort: '',
      network: 'tcp+udp',
    }))
  }

  const handleDelete = (index: number) => {
    setDraftTunnels((prev) => prev.filter((_, i) => i !== index))
  }

  const field = (
    key: 'localAddr' | 'localPort' | 'targetAddr' | 'targetPort',
    placeholder: string,
    label: ReactNode,
  ) => (
    <FormField label={label} sx={FIELD_SX}>
      <TextField
        autoComplete="new-password"
        size="small"
        fullWidth
        type={key.endsWith('Port') ? 'number' : undefined}
        value={values[key]}
        placeholder={placeholder}
        sx={MONO_INPUT}
        onChange={(e) => setValues((v) => ({ ...v, [key]: e.target.value }))}
      />
    </FormField>
  )

  const optional = t('settings.sections.clash.form.fields.tunnels.optional')

  return (
    <BaseDialog
      open={open}
      title={t('settings.sections.clash.form.fields.tunnels.title')}
      dividers
      changes={changes}
      onReset={() => setDraftTunnels(initialTunnels)}
      contentSx={{ width: 532 }}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => {
        setOpen(false)
      }}
      onCancel={() => {
        setOpen(false)
      }}
      onOk={handleSave}
    >
      <FormSection
        title={t('settings.sections.clash.form.fields.tunnels.existing')}
        count={draftTunnels.length}
      />
      {tunnelEntries.length === 0 ? (
        <FormHint>
          {t('settings.sections.clash.form.fields.tunnels.empty')}
        </FormHint>
      ) : (
        tunnelEntries.map((item) => (
          <FormTile key={item.key}>
            <TypeChip>{item.network.join('+')}</TypeChip>
            <Box sx={{ flex: 1, minWidth: 0 }}>
              <Box
                title={`${item.address} → ${item.target}`}
                sx={{
                  ...MONO_TEXT,
                  fontSize: 13.5,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {item.address} → {item.target}
              </Box>
              <Box
                sx={{
                  fontSize: 12.5,
                  color: 'text.secondary',
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {item.proxy ??
                  t('settings.sections.clash.form.fields.tunnels.default')}
              </Box>
            </Box>
            <IconButton
              size="small"
              title={t('shared.actions.delete')}
              sx={{
                color: 'text.secondary',
                '&:hover': { color: 'error.main' },
              }}
              onClick={() => handleDelete(item.index)}
            >
              <DeleteRounded fontSize="small" />
            </IconButton>
          </FormTile>
        ))
      )}

      <FormSection
        title={t('settings.sections.clash.form.fields.tunnels.actions.addNew')}
      />
      <FormRow
        label={t('settings.sections.clash.form.fields.tunnels.protocols')}
      >
        <BaseSegmented
          value={values.network}
          options={NETWORK_OPTIONS}
          onChange={(network) => setValues((v) => ({ ...v, network }))}
        />
      </FormRow>
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
          columnGap: 1.5,
          borderTop: 1,
          borderColor: 'divider',
        }}
      >
        {field(
          'localAddr',
          '127.0.0.1',
          t('settings.sections.clash.form.fields.tunnels.localAddr'),
        )}
        {field(
          'localPort',
          '6553',
          t('settings.sections.clash.form.fields.tunnels.localPort'),
        )}
        {field(
          'targetAddr',
          '8.8.8.8',
          t('settings.sections.clash.form.fields.tunnels.targetAddr'),
        )}
        {field(
          'targetPort',
          '53',
          t('settings.sections.clash.form.fields.tunnels.targetPort'),
        )}

        <FormField
          sx={FIELD_SX}
          label={t('settings.sections.clash.form.fields.tunnels.proxyGroup')}
          optional={optional}
        >
          <Select
            size="small"
            fullWidth
            sx={{ fontSize: 14 }}
            value={values.group}
            displayEmpty
            onChange={(e) => {
              const nextGroup = e.target.value as string

              setValues((v) => ({
                ...v,
                group: nextGroup,
                proxy: nextGroup,
              }))
            }}
          >
            <MenuItem value="">
              {t('settings.sections.clash.form.fields.tunnels.default')}
            </MenuItem>
            {groupNames.map((name) => (
              <MenuItem key={name} value={name}>
                {name}
              </MenuItem>
            ))}
          </Select>
        </FormField>

        <FormField
          sx={FIELD_SX}
          label={t('settings.sections.clash.form.fields.tunnels.proxyNode')}
          optional={optional}
        >
          <Select
            size="small"
            fullWidth
            sx={{ fontSize: 14 }}
            value={values.proxy}
            displayEmpty
            onChange={(e) =>
              setValues((v) => ({
                ...v,
                proxy: e.target.value as string,
              }))
            }
            disabled={!values.group}
          >
            {values.group ? (
              <MenuItem value={values.group}>
                {t('settings.sections.clash.form.fields.tunnels.followGroup')}
              </MenuItem>
            ) : (
              <MenuItem value="">
                {t('settings.sections.clash.form.fields.tunnels.default')}
              </MenuItem>
            )}
            {proxyOptions.map((node) => (
              <MenuItem key={node.name} value={node.name}>
                {node.name}
              </MenuItem>
            ))}
          </Select>
        </FormField>
      </Box>

      <Box
        sx={{
          mt: 0.75,
          mb: 1.25,
          px: 1.25,
          py: 0.75,
          border: '1px dashed',
          borderColor: 'divider',
          borderRadius: '8px',
          ...MONO_TEXT,
          fontSize: 12.5,
          overflowWrap: 'anywhere',
        }}
      >
        <Typography
          component="div"
          color="text.secondary"
          sx={{ fontSize: 11.5, fontWeight: 600, mb: 0.25 }}
        >
          {t('settings.sections.clash.form.fields.tunnels.preview')}
        </Typography>
        {values.network} <Pending value={values.localAddr} hint="127.0.0.1" />:
        <Pending value={values.localPort} hint="6553" />
        {' → '}
        <Pending value={values.targetAddr} hint="8.8.8.8" />:
        <Pending value={values.targetPort} hint="53" />
        {' · '}
        {values.proxy ||
          t('settings.sections.clash.form.fields.tunnels.default')}
      </Box>

      <Button
        fullWidth
        variant="outlined"
        startIcon={<AddRounded />}
        sx={{ mb: 1 }}
        onClick={handleAdd}
      >
        {t('settings.sections.clash.form.fields.tunnels.actions.add')}
      </Button>
    </BaseDialog>
  )
})
