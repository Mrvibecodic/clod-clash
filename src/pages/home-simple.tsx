import CloseRoundedIcon from '@mui/icons-material/CloseRounded'
import HomeWorkRoundedIcon from '@mui/icons-material/HomeWorkRounded'
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import SupportAgentRoundedIcon from '@mui/icons-material/SupportAgentRounded'
import TelegramIcon from '@mui/icons-material/Telegram'
import {
  Alert,
  Avatar,
  Box,
  Button,
  Chip,
  CircularProgress,
  IconButton,
  LinearProgress,
  Stack,
  TextField,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  ConnectButton,
  type ConnectState,
} from '@/components/home/connect-button'
import { ServerSelect, ServerSelectRow } from '@/components/home/server-select'
import { useProfiles } from '@/hooks/use-profiles'
import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useSystemState } from '@/hooks/use-system-state'
import { useVerge } from '@/hooks/use-verge'
import { useUptimeData } from '@/providers/app-data-context'
import {
  createProfile,
  importProfile,
  openWebUrl,
  patchProfile,
  updateProfile,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import parseTraffic from '@/utils/parse-traffic'

/** Colour of the traffic bar: green, then amber, then red as the quota runs out. */
const trafficColor = (usedPercent: number) => {
  if (usedPercent >= 90) return 'error' as const
  if (usedPercent >= 70) return 'warning' as const
  return 'success' as const
}

const DAY = 24 * 60 * 60

const HomeSimplePage = () => {
  const { t } = useTranslation()
  const { current, mutateProfiles } = useProfiles()
  const { verge, patchVerge } = useVerge()
  const { isTunModeAvailable } = useSystemState()
  const { uptime } = useUptimeData()
  const { indicator: sysproxyOn, toggleSystemProxy } = useSystemProxyState()

  const [busy, setBusy] = useState(false)
  const [errorText, setErrorText] = useState<string>()
  const [serverOpen, setServerOpen] = useState(false)
  const [subUrl, setSubUrl] = useState('')
  const [refreshing, setRefreshing] = useState(false)

  const tunMode = verge?.main_switch_mode === 'tun'
  const connected = tunMode ? Boolean(verge?.enable_tun_mode) : sysproxyOn

  const state: ConnectState = errorText
    ? 'error'
    : busy
      ? 'connecting'
      : connected
        ? 'on'
        : 'off'

  const toggle = useLockFn(async () => {
    setBusy(true)
    setErrorText(undefined)
    try {
      if (tunMode) {
        if (!isTunModeAvailable && !verge?.enable_tun_mode) {
          throw new Error(t('home.components.connect.errors.serviceRequired'))
        }
        await patchVerge({ enable_tun_mode: !verge?.enable_tun_mode })
      } else {
        await toggleSystemProxy(!sysproxyOn)
      }
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : String(error))
    } finally {
      setBusy(false)
    }
  })

  const refresh = useLockFn(async () => {
    if (!current?.uid) return
    setRefreshing(true)
    try {
      await updateProfile(current.uid)
      await mutateProfiles()
    } catch (error) {
      showNotice.error(error)
    } finally {
      setRefreshing(false)
    }
  })

  const addSubscription = useLockFn(async () => {
    const url = subUrl.trim()
    if (!url) return
    try {
      await importProfile(url)
      await mutateProfiles()
      setSubUrl('')
    } catch {
      // A URL that is not a subscription is still a perfectly normal mistake;
      // fall back to creating a remote profile so the error names the cause.
      try {
        await createProfile({ type: 'remote', name: url, url })
        await mutateProfiles()
        setSubUrl('')
      } catch (error) {
        showNotice.error(error)
      }
    }
  })

  const dismissAnnounce = useCallback(async () => {
    if (!current?.uid || !current.announce) return
    try {
      // The backend stores a hash of the dismissed text, so a new announcement
      // shows up again by itself.
      await patchProfile(current.uid, { announce_seen_hash: 'dismissed' })
      await mutateProfiles()
    } catch (error) {
      showNotice.error(error)
    }
  }, [current?.uid, current?.announce, mutateProfiles])

  const subscription = useMemo(() => {
    const extra = current?.extra
    if (!extra) return undefined

    const used = extra.upload + extra.download
    const unlimited = extra.total === 0
    const forever = !extra.expire
    const usedPercent = unlimited
      ? 0
      : Math.min(100, Math.round((used * 100) / extra.total))
    const daysLeft = forever
      ? undefined
      : Math.max(0, Math.ceil((extra.expire - Date.now() / 1000) / DAY))

    return {
      used,
      unlimited,
      forever,
      usedPercent,
      daysLeft,
      total: extra.total,
    }
  }, [current?.extra])

  const openLink = useCallback(async (url?: string) => {
    if (!url) return
    try {
      await openWebUrl(url)
    } catch (error) {
      showNotice.error(error)
    }
  }, [])

  // No subscription yet: the only thing worth showing is how to add one.
  if (!current) {
    return (
      <Stack
        sx={{
          height: '100%',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 2,
          px: 4,
        }}
      >
        <Typography variant="h5">{t('home.pages.simple.welcome')}</Typography>
        <Typography color="text.secondary" sx={{ textAlign: 'center' }}>
          {t('home.pages.simple.welcomeHint')}
        </Typography>
        <Stack direction="row" sx={{ gap: 1, width: '100%', maxWidth: 480 }}>
          <TextField
            fullWidth
            size="small"
            value={subUrl}
            onChange={(event) => setSubUrl(event.target.value)}
            placeholder={t('home.pages.simple.subscriptionPlaceholder')}
          />
          <Button
            variant="contained"
            disabled={!subUrl.trim()}
            onClick={() => void addSubscription()}
          >
            {t('shared.actions.new')}
          </Button>
        </Stack>
      </Stack>
    )
  }

  const supportIsTelegram =
    current.support_url?.includes('t.me/') ||
    current.support_url?.startsWith('tg:')

  return (
    <Stack sx={{ height: '100%', overflowY: 'auto', p: 2, gap: 2 }}>
      {/* provider identity */}
      <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
        {current.logo ? (
          <Avatar src={current.logo} alt="" sx={{ width: 40, height: 40 }} />
        ) : null}
        <Typography variant="h6" noWrap sx={{ flex: 1, minWidth: 0 }}>
          {current.name}
        </Typography>
        <IconButton
          onClick={() => void refresh()}
          disabled={refreshing}
          aria-label={t('shared.actions.refresh')}
        >
          {refreshing ? <CircularProgress size={20} /> : <RefreshRoundedIcon />}
        </IconButton>
      </Stack>

      {/* provider announcement */}
      {current.announce && !current.announce_seen_hash ? (
        <Alert
          severity="info"
          onClick={() => void openLink(current.announce_url)}
          sx={{
            whiteSpace: 'pre-line',
            cursor: current.announce_url ? 'pointer' : 'default',
          }}
          action={
            <IconButton
              size="small"
              aria-label={t('shared.actions.close')}
              onClick={(event) => {
                event.stopPropagation()
                void dismissAnnounce()
              }}
            >
              <CloseRoundedIcon fontSize="small" />
            </IconButton>
          }
        >
          {current.announce}
        </Alert>
      ) : null}

      <Box sx={{ display: 'flex', justifyContent: 'center', py: 2 }}>
        <ConnectButton
          state={state}
          uptime={uptime}
          errorText={errorText}
          onToggle={() => void toggle()}
        />
      </Box>

      <ServerSelectRow onOpen={() => setServerOpen(true)} />
      <ServerSelect open={serverOpen} onClose={() => setServerOpen(false)} />

      {/* subscription card; hidden entirely when there is nothing to report */}
      {subscription && !(subscription.unlimited && subscription.forever) ? (
        <Stack
          sx={{
            gap: 1,
            p: 2,
            borderRadius: 2,
            border: (theme) => `1px solid ${theme.palette.divider}`,
          }}
        >
          <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
            <Typography variant="body2" sx={{ flex: 1 }}>
              {parseTraffic(subscription.used)} /{' '}
              {subscription.unlimited
                ? t('profiles.components.profileItem.labels.unlimited')
                : parseTraffic(subscription.total)}
            </Typography>
            {subscription.daysLeft !== undefined ? (
              <Chip
                size="small"
                color={subscription.daysLeft <= 3 ? 'error' : 'default'}
                label={t('home.pages.simple.daysLeft', {
                  count: subscription.daysLeft,
                })}
              />
            ) : (
              <Chip
                size="small"
                label={t('profiles.components.profileItem.labels.neverExpires')}
              />
            )}
          </Stack>

          {subscription.unlimited ? null : (
            <LinearProgress
              variant="determinate"
              value={subscription.usedPercent}
              color={trafficColor(subscription.usedPercent)}
            />
          )}

          {current.refill_date ? (
            <Typography variant="caption" color="text.secondary">
              {t('profiles.components.profileItem.tooltips.refillDate', {
                date: dayjs(current.refill_date * 1000).format('YYYY-MM-DD'),
              })}
            </Typography>
          ) : null}
        </Stack>
      ) : null}

      <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
        {current.home ? (
          <Button
            startIcon={<HomeWorkRoundedIcon />}
            onClick={() => void openLink(current.home)}
          >
            {t('home.pages.simple.portal')}
          </Button>
        ) : null}
        {current.support_url ? (
          <Button
            startIcon={
              supportIsTelegram ? <TelegramIcon /> : <SupportAgentRoundedIcon />
            }
            onClick={() => void openLink(current.support_url)}
          >
            {t('profiles.components.hwidDialog.support')}
          </Button>
        ) : null}
      </Stack>
    </Stack>
  )
}

export default HomeSimplePage
