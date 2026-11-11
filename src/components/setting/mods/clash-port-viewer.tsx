import { Shuffle } from '@mui/icons-material'
import {
  Box,
  CircularProgress,
  IconButton,
  Stack,
  TextField,
} from '@mui/material'
import { useLockFn, useRequest } from 'ahooks'
import { type Ref, useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog, FormRow, Switch } from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { useChangeCount } from '@/hooks/use-change-count'
import { type ClashInfoPatch, useClash, useClashInfo } from '@/hooks/use-clash'
import { useVerge } from '@/hooks/use-verge'
import { getCoreLadder, isPortInUse } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { setCacheData } from '@/services/query-client'
import getSystem from '@/utils/get-system'
import {
  findDuplicatePort,
  findPortOutOfRange,
  MAX_PORT,
  MIN_PORT,
} from '@/utils/ports'

const OS = getSystem()

type Ports = {
  mixedFollowsSubscription: boolean
  mixedPort: number
  socksPort: number
  socksEnabled: boolean
  httpPort: number
  httpEnabled: boolean
  redirPort: number
  redirEnabled: boolean
  tproxyPort: number
  tproxyEnabled: boolean
}

interface ClashPortViewerRef {
  open: () => void
  close: () => void
}

const PORT_SX = {
  width: 84,
  ...MONO_INPUT,
  '& input': { textAlign: 'center' },
}

const parsePort = (text: string) => +text.replace(/\D+/g, '').slice(0, 5)

const generateRandomPort = () =>
  Math.floor(Math.random() * (65535 - 1025 + 1)) + 1025

export function ClashPortViewer({ ref }: { ref?: Ref<ClashPortViewerRef> }) {
  const { t } = useTranslation()
  const { clashInfo, patchInfo } = useClashInfo()
  const { ladder } = useClash()
  const { verge, patchVerge } = useVerge()
  const [open, setOpen] = useState(false)

  const portsFrom = (shownLadder: ICoreLadder | undefined): Ports => ({
    mixedFollowsSubscription: shownLadder?.mixed_port == null,
    mixedPort: shownLadder?.mixed_port ?? clashInfo?.mixed_port ?? 7897,
    socksPort: verge?.verge_socks_port || 7898,
    socksEnabled: verge?.verge_socks_enabled ?? false,
    httpPort: verge?.verge_port || 7899,
    httpEnabled: verge?.verge_http_enabled ?? false,
    redirPort: verge?.verge_redir_port || 7895,
    redirEnabled: verge?.verge_redir_enabled ?? false,
    tproxyPort: verge?.verge_tproxy_port || 7896,
    tproxyEnabled: verge?.verge_tproxy_enabled ?? false,
  })

  // Что в окне сейчас и с чем сравнивать: то, что открыли, или то, что уже
  // принято ядром, если сохранилась только часть.
  const [ports, setPorts] = useState<Ports>(() => portsFrom(ladder))
  const [initial, setInitial] = useState<Ports>(ports)
  const [ladderRead, setLadderRead] = useState(false)
  const [subscriptionPort, setSubscriptionPort] = useState<number | undefined>(
    undefined,
  )
  const setPort = <K extends keyof Ports>(key: K, value: Ports[K]) =>
    setPorts((prev) => ({ ...prev, [key]: value }))
  const {
    mixedFollowsSubscription,
    mixedPort,
    socksPort,
    socksEnabled,
    httpPort,
    httpEnabled,
    redirPort,
    redirEnabled,
    tproxyPort,
    tproxyEnabled,
  } = ports
  const changes = useChangeCount(initial, ports)

  // Запрос на сохранение, предотвращает зависание GUI
  const { loading, run: saveSettings } = useRequest(
    async (params: {
      clashConfig: ClashInfoPatch
      vergeConfig: Partial<IVergeConfig>
      appliedPorts: Partial<Ports>
    }) => {
      const { clashConfig, vergeConfig, appliedPorts } = params
      await patchInfo(clashConfig)
      setInitial((prev) => ({ ...prev, ...appliedPorts }))
      await patchVerge(vergeConfig)
    },
    {
      manual: true,
      onSuccess: () => {
        setOpen(false)
        showNotice.success('settings.modals.clashPort.messages.saved')
      },
      onError: (error) => {
        showNotice.error('settings.modals.clashPort.messages.saveFailed', error)
      },
    },
  )

  useImperativeHandle(ref, () => ({
    open: async () => {
      // clod:port-ladder — лесенка могла протухнуть: читаем её заново ДО того,
      // как заполнить поля, иначе диалог показал бы и сохранил старое
      // закрепление.
      setLadderRead(false)
      let freshLadder: ICoreLadder | undefined
      try {
        freshLadder = await getCoreLadder()
        setCacheData(['getCoreLadder'], freshLadder)
      } catch {
        showNotice.error('settings.modals.clashPort.messages.ladderUnread')
      }
      const opened = portsFrom(freshLadder ?? ladder)
      setPorts(opened)
      setInitial(opened)
      setSubscriptionPort(
        freshLadder !== undefined && freshLadder.mixed_port == null
          ? clashInfo?.mixed_port
          : undefined,
      )
      setLadderRead(freshLadder !== undefined)
      setOpen(true)
    },
    close: () => setOpen(false),
  }))

  // TODO снизить сложность кода, затраты на производительность
  const onSave = useLockFn(async () => {
    // Проверка диапазона портов
    const outOfRange = findPortOutOfRange([
      ...(!ladderRead || mixedFollowsSubscription ? [] : [mixedPort]),
      ...(socksEnabled ? [socksPort] : []),
      ...(httpEnabled ? [httpPort] : []),
      ...(redirEnabled ? [redirPort] : []),
      ...(tproxyEnabled ? [tproxyPort] : []),
    ])

    if (outOfRange) {
      showNotice.error(
        outOfRange.verdict === 'tooLow'
          ? 'settings.modals.clashPort.messages.portTooLow'
          : 'settings.modals.clashPort.messages.portTooHigh',
        outOfRange.verdict === 'tooLow' ? { min: MIN_PORT } : { max: MAX_PORT },
      )
      return
    }

    // Проверка конфликта портов
    const effectiveMixed = !ladderRead
      ? -1
      : mixedFollowsSubscription
        ? (subscriptionPort ?? -1)
        : mixedPort
    const portList = [
      effectiveMixed,
      socksEnabled ? socksPort : -1,
      httpEnabled ? httpPort : -1,
      redirEnabled ? redirPort : -1,
      tproxyEnabled ? tproxyPort : -1,
    ].filter((p) => p !== -1)

    const duplicate = findDuplicatePort(portList)
    if (duplicate !== undefined) {
      showNotice.error('settings.modals.clashPort.messages.portInUse', {
        port: duplicate,
      })
      return
    }

    const original = initial
    const clashConfig: ClashInfoPatch = {}
    const vergeConfig: Partial<IVergeConfig> = {}
    const appliedPorts: Partial<Ports> = {}
    const listeners = [
      {
        name: 'socks',
        clashKey: 'socks-port',
        portKey: 'verge_socks_port',
        enabledKey: 'verge_socks_enabled',
        port: socksPort,
        enabled: socksEnabled,
      },
      {
        name: 'http',
        clashKey: 'port',
        portKey: 'verge_port',
        enabledKey: 'verge_http_enabled',
        port: httpPort,
        enabled: httpEnabled,
      },
      {
        name: 'redir',
        clashKey: 'redir-port',
        portKey: 'verge_redir_port',
        enabledKey: 'verge_redir_enabled',
        port: redirPort,
        enabled: redirEnabled,
      },
      {
        name: 'tproxy',
        clashKey: 'tproxy-port',
        portKey: 'verge_tproxy_port',
        enabledKey: 'verge_tproxy_enabled',
        port: tproxyPort,
        enabled: tproxyEnabled,
      },
    ] as const

    for (const listener of listeners) {
      const wasEnabled = original[`${listener.name}Enabled`]
      if (listener.enabled !== wasEnabled) {
        vergeConfig[listener.enabledKey] = listener.enabled
      }
      if (!listener.enabled) continue
      if (!wasEnabled || listener.port !== original[`${listener.name}Port`]) {
        clashConfig[listener.clashKey] = listener.port
        appliedPorts[`${listener.name}Port`] = listener.port
      }
      if (listener.port !== verge?.[listener.portKey]) {
        vergeConfig[listener.portKey] = listener.port
      }
    }

    if (ladderRead) {
      const mixedChanged = mixedFollowsSubscription
        ? !original.mixedFollowsSubscription
        : original.mixedFollowsSubscription || mixedPort !== original.mixedPort
      if (mixedChanged) {
        clashConfig['mixed-port'] = mixedFollowsSubscription
          ? 'auto'
          : mixedPort
        vergeConfig.verge_mixed_port = mixedFollowsSubscription ? 0 : mixedPort
        appliedPorts.mixedFollowsSubscription = mixedFollowsSubscription
        appliedPorts.mixedPort = mixedFollowsSubscription
          ? subscriptionPort
          : mixedPort
      }
    }

    if (
      Object.keys(clashConfig).length === 0 &&
      Object.keys(vergeConfig).length === 0
    ) {
      setOpen(false)
      return
    }

    for (const [key, port] of Object.entries(clashConfig)) {
      if (typeof port !== 'number') continue
      if (key === 'mixed-port' && port === original.mixedPort) continue
      try {
        if (await isPortInUse(port)) {
          showNotice.error('settings.modals.clashPort.messages.portInUse', {
            port,
          })
          return
        }
      } catch (error) {
        showNotice.error(error)
        return
      }
    }

    // Отправляем запрос на сохранение
    saveSettings({ clashConfig, vergeConfig, appliedPorts })
  })

  const portRow = ({
    label,
    help,
    port,
    setPort,
    enabled,
    setEnabled,
  }: {
    label: string
    help?: string
    port: number
    setPort: (port: number) => void
    enabled: boolean
    setEnabled: (enabled: boolean) => void
  }) => (
    <FormRow label={label} help={help}>
      <TextField
        size="small"
        sx={PORT_SX}
        value={port}
        onChange={(e) => setPort(parsePort(e.target.value))}
        disabled={!enabled}
      />
      <IconButton
        size="small"
        onClick={() => setPort(generateRandomPort())}
        title={t('settings.modals.clashPort.actions.random')}
        disabled={!enabled}
      >
        <Shuffle fontSize="small" />
      </IconButton>
      <Switch
        size="small"
        checked={enabled}
        onChange={(_, c) => setEnabled(c)}
      />
    </FormRow>
  )

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.clashPort.title')}
      dividers
      changes={changes}
      onReset={() => setPorts(initial)}
      contentSx={{ width: 472 }}
      loading={loading}
      okBtn={
        loading ? (
          <Stack direction="row" spacing={1} sx={{ alignItems: 'center' }}>
            <CircularProgress size={20} />
            {t('shared.statuses.saving')}
          </Stack>
        ) : (
          t('shared.actions.save')
        )
      }
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <FormRow
        label={t('settings.modals.clashPort.fields.mixed')}
        help={
          mixedFollowsSubscription
            ? t('settings.modals.clashPort.fields.subscriptionPortUnknown')
            : undefined
        }
        disabled={mixedFollowsSubscription}
      >
        <TextField
          size="small"
          sx={PORT_SX}
          value={
            mixedFollowsSubscription ? (subscriptionPort ?? '') : mixedPort
          }
          placeholder={mixedFollowsSubscription ? '—' : undefined}
          disabled={mixedFollowsSubscription || !ladderRead}
          onChange={(e) => setPort('mixedPort', parsePort(e.target.value))}
        />
        <IconButton
          size="small"
          disabled={mixedFollowsSubscription || !ladderRead}
          onClick={() => setPort('mixedPort', generateRandomPort())}
          title={t('settings.modals.clashPort.actions.random')}
        >
          <Shuffle fontSize="small" />
        </IconButton>
        {/* Место выключателя соседних строк: у смешанного порта его нет. */}
        <Box sx={{ width: 40, flex: 'none' }} />
      </FormRow>

      <FormRow
        label={t('settings.modals.clashPort.fields.mixedFollowsSubscription')}
      >
        <Switch
          size="small"
          checked={mixedFollowsSubscription}
          disabled={!ladderRead}
          onChange={(_, checked) =>
            setPort('mixedFollowsSubscription', checked)
          }
        />
      </FormRow>

      {portRow({
        label: t('settings.modals.clashPort.fields.socks'),
        port: socksPort,
        setPort: (port) => setPort('socksPort', port),
        enabled: socksEnabled,
        setEnabled: (on) => setPort('socksEnabled', on),
      })}
      {portRow({
        label: t('settings.modals.clashPort.fields.http'),
        port: httpPort,
        setPort: (port) => setPort('httpPort', port),
        enabled: httpEnabled,
        setEnabled: (on) => setPort('httpEnabled', on),
      })}
      {OS !== 'windows' &&
        portRow({
          label: t('settings.modals.clashPort.fields.redir'),
          help: t('settings.modals.clashPort.messages.firewallRequired'),
          port: redirPort,
          setPort: (port) => setPort('redirPort', port),
          enabled: redirEnabled,
          setEnabled: (on) => setPort('redirEnabled', on),
        })}
      {OS === 'linux' &&
        portRow({
          label: t('settings.modals.clashPort.fields.tproxy'),
          help: t('settings.modals.clashPort.messages.firewallRequired'),
          port: tproxyPort,
          setPort: (port) => setPort('tproxyPort', port),
          enabled: tproxyEnabled,
          setEnabled: (on) => setPort('tproxyEnabled', on),
        })}
    </BaseDialog>
  )
}
