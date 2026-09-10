import { Shuffle } from '@mui/icons-material'
import {
  CircularProgress,
  IconButton,
  List,
  ListItem,
  ListItemText,
  Stack,
  TextField,
} from '@mui/material'
import { useLockFn, useRequest } from 'ahooks'
import { forwardRef, useImperativeHandle, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog, Switch } from '@/components/base'
import { useClash, useClashInfo } from '@/hooks/use-clash'
import { useVerge } from '@/hooks/use-verge'
import { isPortInUse } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { revalidateQuery } from '@/services/query-client'
import getSystem from '@/utils/get-system'
import {
  findDuplicatePort,
  findPortOutOfRange,
  MAX_PORT,
  MIN_PORT,
} from '@/utils/ports'

const OS = getSystem()

interface ClashPortViewerRef {
  open: () => void
  close: () => void
}

const generateRandomPort = () =>
  Math.floor(Math.random() * (65535 - 1025 + 1)) + 1025

export const ClashPortViewer = forwardRef<ClashPortViewerRef>((_, ref) => {
  const { t } = useTranslation()
  const { clashInfo, patchInfo } = useClashInfo()
  const { ladder } = useClash()
  const { verge, patchVerge } = useVerge()
  const [open, setOpen] = useState(false)

  // Mixed Port
  // clod:port-ladder — «как в подписке» это отсутствие порта у нас. Число в поле
  // берётся из порта, на котором ядро слушает, и только если закрепления не было
  // уже в момент открытия: пока закрепление живо, порт подписки нам неизвестен.
  const [mixedFollowsSubscription, setMixedFollowsSubscription] = useState(
    ladder?.mixed_port == null,
  )
  const [mixedPort, setMixedPort] = useState(
    ladder?.mixed_port ?? clashInfo?.mixed_port ?? 7897,
  )
  const [ladderRead, setLadderRead] = useState(false)
  const [subscriptionPort, setSubscriptionPort] = useState<number | undefined>(
    undefined,
  )

  // Состояние остальных портов
  const [socksPort, setSocksPort] = useState(verge?.verge_socks_port ?? 7898)
  const [socksEnabled, setSocksEnabled] = useState(
    verge?.verge_socks_enabled ?? false,
  )
  const [httpPort, setHttpPort] = useState(verge?.verge_port ?? 7899)
  const [httpEnabled, setHttpEnabled] = useState(
    verge?.verge_http_enabled ?? false,
  )
  const [redirPort, setRedirPort] = useState(verge?.verge_redir_port ?? 7895)
  const [redirEnabled, setRedirEnabled] = useState(
    verge?.verge_redir_enabled ?? false,
  )
  const [tproxyPort, setTproxyPort] = useState(verge?.verge_tproxy_port ?? 7896)
  const [tproxyEnabled, setTproxyEnabled] = useState(
    verge?.verge_tproxy_enabled ?? false,
  )

  // Сохраняем исходные значения при открытии диалога, чтобы восстановить их при обнаружении занятого порта
  const originalPortsRef = useRef<Record<string, any> | null>(null)

  // Запрос на сохранение, предотвращает зависание GUI
  const { loading, run: saveSettings } = useRequest(
    async (params: { clashConfig: any; vergeConfig: any }) => {
      const { clashConfig, vergeConfig } = params
      await patchInfo(clashConfig)
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
      // закрепление. Если ответа нет, кнопка сохранения ниже остаётся
      // заблокированной — умолчание тумблера не должно снимать закрепление,
      // которого не успели прочитать.
      setLadderRead(false)
      const freshLadder = (await revalidateQuery(['getCoreLadder']).catch(
        () => undefined,
      )) as ICoreLadder | undefined
      if (freshLadder === undefined) {
        showNotice.error('settings.modals.clashPort.messages.ladderUnread')
      }
      const shownLadder = freshLadder ?? ladder
      originalPortsRef.current = {
        mixedFollowsSubscription: shownLadder?.mixed_port == null,
        mixedPort: shownLadder?.mixed_port ?? clashInfo?.mixed_port ?? 7897,
        socksPort: verge?.verge_socks_port ?? 7898,
        socksEnabled: verge?.verge_socks_enabled ?? false,
        httpPort: verge?.verge_port ?? 7899,
        httpEnabled: verge?.verge_http_enabled ?? false,
        redirPort: verge?.verge_redir_port ?? 7895,
        redirEnabled: verge?.verge_redir_enabled ?? false,
        tproxyPort: verge?.verge_tproxy_port ?? 7896,
        tproxyEnabled: verge?.verge_tproxy_enabled ?? false,
      }

      setMixedFollowsSubscription(
        originalPortsRef.current.mixedFollowsSubscription,
      )
      setMixedPort(originalPortsRef.current.mixedPort)
      setSocksPort(originalPortsRef.current.socksPort)
      setSocksEnabled(originalPortsRef.current.socksEnabled)
      setHttpPort(originalPortsRef.current.httpPort)
      setHttpEnabled(originalPortsRef.current.httpEnabled)
      setRedirPort(originalPortsRef.current.redirPort)
      setRedirEnabled(originalPortsRef.current.redirEnabled)
      setTproxyPort(originalPortsRef.current.tproxyPort)
      setTproxyEnabled(originalPortsRef.current.tproxyEnabled)
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
    // Проверка конфликта портов
    // clod:port-ladder — при «как в подписке» с остальными портами сверяется
    // порт подписки, и только когда он прочитан: два слушателя на одном порту
    // ядро не поднимет, а сверка с чужим числом врёт в обе стороны.
    const effectiveMixed = mixedFollowsSubscription
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

    // Проверка диапазона портов
    const outOfRange = findPortOutOfRange([
      mixedFollowsSubscription ? 0 : mixedPort,
      socksPort,
      httpPort,
      redirPort,
      tproxyPort,
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

    const original = originalPortsRef.current
    const changedPorts: number[] = []

    if (!mixedFollowsSubscription && mixedPort !== original?.mixedPort)
      changedPorts.push(mixedPort)
    if (socksEnabled && socksPort !== original?.socksPort)
      changedPorts.push(socksPort)
    if (httpEnabled && httpPort !== original?.httpPort)
      changedPorts.push(httpPort)
    if (redirEnabled && redirPort !== original?.redirPort)
      changedPorts.push(redirPort)
    if (tproxyEnabled && tproxyPort !== original?.tproxyPort)
      changedPorts.push(tproxyPort)

    for (const port of changedPorts) {
      try {
        const inUse = await isPortInUse(port)
        if (inUse) {
          showNotice.error('settings.modals.clashPort.messages.portInUse', {
            port,
          })
          if (original) {
            setMixedFollowsSubscription(original.mixedFollowsSubscription)
            setMixedPort(original.mixedPort)
            setSocksPort(original.socksPort)
            setSocksEnabled(original.socksEnabled)
            setHttpPort(original.httpPort)
            setHttpEnabled(original.httpEnabled)
            setRedirPort(original.redirPort)
            setRedirEnabled(original.redirEnabled)
            setTproxyPort(original.tproxyPort)
            setTproxyEnabled(original.tproxyEnabled)
          } else {
            setMixedFollowsSubscription(ladder?.mixed_port == null)
            setMixedPort(ladder?.mixed_port ?? clashInfo?.mixed_port ?? 7897)
            setSocksPort(verge?.verge_socks_port ?? 7898)
            setSocksEnabled(verge?.verge_socks_enabled ?? false)
            setHttpPort(verge?.verge_port ?? 7899)
            setHttpEnabled(verge?.verge_http_enabled ?? false)
            setRedirPort(verge?.verge_redir_port ?? 7895)
            setRedirEnabled(verge?.verge_redir_enabled ?? false)
            setTproxyPort(verge?.verge_tproxy_port ?? 7896)
            setTproxyEnabled(verge?.verge_tproxy_enabled ?? false)
          }
          return
        }
      } catch (error) {
        showNotice.error(error)
        return
      }
    }

    // Готовим данные конфига
    const clashConfig = {
      'mixed-port': mixedFollowsSubscription ? 'auto' : mixedPort,
      'socks-port': socksPort,
      port: httpPort,
      'redir-port': redirPort,
      'tproxy-port': tproxyPort,
    }

    // clod:port-ladder — ноль снимает закрепление: иначе старый порт жил бы в
    // файле настроек вечно и служил запасным там, где его давно нет.
    const vergeConfig = {
      verge_mixed_port: mixedFollowsSubscription ? 0 : mixedPort,
      verge_socks_port: socksPort,
      verge_socks_enabled: socksEnabled,
      verge_port: httpPort,
      verge_http_enabled: httpEnabled,
      verge_redir_port: redirPort,
      verge_redir_enabled: redirEnabled,
      verge_tproxy_port: tproxyPort,
      verge_tproxy_enabled: tproxyEnabled,
    }

    // Отправляем запрос на сохранение
    saveSettings({ clashConfig, vergeConfig })
  })

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.clashPort.title')}
      contentSx={{
        width: 400,
      }}
      loading={loading || !ladderRead}
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
      <List sx={{ width: '100%' }}>
        <ListItem sx={{ padding: '4px 0', minHeight: 36 }}>
          <ListItemText
            primary={t('settings.modals.clashPort.fields.mixed')}
            slotProps={{ primary: { sx: { fontSize: 12 } } }}
          />
          <div style={{ display: 'flex', alignItems: 'center' }}>
            <TextField
              size="small"
              sx={{ width: 80, mr: 0.5, fontSize: 12 }}
              value={
                mixedFollowsSubscription
                  ? (subscriptionPort ?? '')
                  : mixedPort
              }
              placeholder={
                mixedFollowsSubscription
                  ? t(
                      'settings.modals.clashPort.fields.subscriptionPortUnknown',
                    )
                  : undefined
              }
              disabled={mixedFollowsSubscription}
              onChange={(e) =>
                setMixedPort(+e.target.value?.replace(/\D+/, '').slice(0, 5))
              }
              slotProps={{ htmlInput: { style: { fontSize: 12 } } }}
            />
            <IconButton
              size="small"
              disabled={mixedFollowsSubscription}
              onClick={() => setMixedPort(generateRandomPort())}
              title={t('settings.modals.clashPort.actions.random')}
              sx={{ mr: 0.5 }}
            >
              <Shuffle fontSize="small" />
            </IconButton>
            <Switch
              size="small"
              checked={true}
              disabled={true}
              sx={{ ml: 0.5, opacity: 0.7 }}
            />
          </div>
        </ListItem>

        <ListItem sx={{ padding: '4px 0', minHeight: 36 }}>
          <ListItemText
            primary={t(
              'settings.modals.clashPort.fields.mixedFollowsSubscription',
            )}
            slotProps={{ primary: { sx: { fontSize: 12 } } }}
          />
          <Switch
            size="small"
            checked={mixedFollowsSubscription}
            onChange={(_, checked) => setMixedFollowsSubscription(checked)}
          />
        </ListItem>

        <ListItem sx={{ padding: '4px 0', minHeight: 36 }}>
          <ListItemText
            primary={t('settings.modals.clashPort.fields.socks')}
            slotProps={{ primary: { sx: { fontSize: 12 } } }}
          />
          <div style={{ display: 'flex', alignItems: 'center' }}>
            <TextField
              size="small"
              sx={{ width: 80, mr: 0.5, fontSize: 12 }}
              value={socksPort}
              onChange={(e) =>
                setSocksPort(+e.target.value?.replace(/\D+/, '').slice(0, 5))
              }
              disabled={!socksEnabled}
              slotProps={{ htmlInput: { style: { fontSize: 12 } } }}
            />
            <IconButton
              size="small"
              onClick={() => setSocksPort(generateRandomPort())}
              title={t('settings.modals.clashPort.actions.random')}
              disabled={!socksEnabled}
              sx={{ mr: 0.5 }}
            >
              <Shuffle fontSize="small" />
            </IconButton>
            <Switch
              size="small"
              checked={socksEnabled}
              onChange={(_, c) => setSocksEnabled(c)}
              sx={{ ml: 0.5 }}
            />
          </div>
        </ListItem>

        <ListItem sx={{ padding: '4px 0', minHeight: 36 }}>
          <ListItemText
            primary={t('settings.modals.clashPort.fields.http')}
            slotProps={{ primary: { sx: { fontSize: 12 } } }}
          />
          <div style={{ display: 'flex', alignItems: 'center' }}>
            <TextField
              size="small"
              sx={{ width: 80, mr: 0.5, fontSize: 12 }}
              value={httpPort}
              onChange={(e) =>
                setHttpPort(+e.target.value?.replace(/\D+/, '').slice(0, 5))
              }
              disabled={!httpEnabled}
              slotProps={{ htmlInput: { style: { fontSize: 12 } } }}
            />
            <IconButton
              size="small"
              onClick={() => setHttpPort(generateRandomPort())}
              title={t('settings.modals.clashPort.actions.random')}
              disabled={!httpEnabled}
              sx={{ mr: 0.5 }}
            >
              <Shuffle fontSize="small" />
            </IconButton>
            <Switch
              size="small"
              checked={httpEnabled}
              onChange={(_, c) => setHttpEnabled(c)}
              sx={{ ml: 0.5 }}
            />
          </div>
        </ListItem>

        {OS !== 'windows' && (
          <ListItem sx={{ padding: '4px 0', minHeight: 36 }}>
            <ListItemText
              primary={t('settings.modals.clashPort.fields.redir')}
              slotProps={{ primary: { sx: { fontSize: 12 } } }}
            />
            <div style={{ display: 'flex', alignItems: 'center' }}>
              <TextField
                size="small"
                sx={{ width: 80, mr: 0.5, fontSize: 12 }}
                value={redirPort}
                onChange={(e) =>
                  setRedirPort(+e.target.value?.replace(/\D+/, '').slice(0, 5))
                }
                disabled={!redirEnabled}
                slotProps={{ htmlInput: { style: { fontSize: 12 } } }}
              />
              <IconButton
                size="small"
                onClick={() => setRedirPort(generateRandomPort())}
                title={t('settings.modals.clashPort.actions.random')}
                disabled={!redirEnabled}
                sx={{ mr: 0.5 }}
              >
                <Shuffle fontSize="small" />
              </IconButton>
              <Switch
                size="small"
                checked={redirEnabled}
                onChange={(_, c) => setRedirEnabled(c)}
                sx={{ ml: 0.5 }}
              />
            </div>
          </ListItem>
        )}

        {OS === 'linux' && (
          <ListItem sx={{ padding: '4px 0', minHeight: 36 }}>
            <ListItemText
              primary={t('settings.modals.clashPort.fields.tproxy')}
              slotProps={{ primary: { sx: { fontSize: 12 } } }}
            />
            <div style={{ display: 'flex', alignItems: 'center' }}>
              <TextField
                size="small"
                sx={{ width: 80, mr: 0.5, fontSize: 12 }}
                value={tproxyPort}
                onChange={(e) =>
                  setTproxyPort(+e.target.value?.replace(/\D+/, '').slice(0, 5))
                }
                disabled={!tproxyEnabled}
                slotProps={{ htmlInput: { style: { fontSize: 12 } } }}
              />
              <IconButton
                size="small"
                onClick={() => setTproxyPort(generateRandomPort())}
                title={t('settings.modals.clashPort.actions.random')}
                disabled={!tproxyEnabled}
                sx={{ mr: 0.5 }}
              >
                <Shuffle fontSize="small" />
              </IconButton>
              <Switch
                size="small"
                checked={tproxyEnabled}
                onChange={(_, c) => setTproxyEnabled(c)}
                sx={{ ml: 0.5 }}
              />
            </div>
          </ListItem>
        )}
      </List>
    </BaseDialog>
  )
})
