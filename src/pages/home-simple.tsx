import HomeWorkRoundedIcon from '@mui/icons-material/HomeWorkRounded'
import SupportAgentRoundedIcon from '@mui/icons-material/SupportAgentRounded'
import TelegramIcon from '@mui/icons-material/Telegram'
import { alpha, Box, Button, Stack, TextField, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  ConnectButton,
  type ConnectState,
} from '@/components/home/connect-button'
import { ModeStatus } from '@/components/home/mode-status'
import { ProviderBanners } from '@/components/home/provider-banners'
import { ProviderHeader } from '@/components/home/provider-header'
import { ServerSelect, ServerSelectRow } from '@/components/home/server-select'
import { SessionTraffic } from '@/components/home/session-traffic'
import { SubscriptionCard } from '@/components/home/subscription-card'
import { TunStatus } from '@/components/home/tun-status'
import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useProfiles } from '@/hooks/use-profiles'
import { useSessionUptime } from '@/hooks/use-session-uptime'
import { useSimpleMode } from '@/hooks/use-simple-mode'
import {
  createProfile,
  enhanceProfiles,
  importProfile,
  openWebUrl,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

const HomeSimplePage = () => {
  const { t } = useTranslation()
  const { current, mutateProfiles } = useProfiles()
  const { connected, toggleConnection } = useConnectTargets()
  const { setSimpleMode } = useSimpleMode()
  const uptime = useSessionUptime(connected)

  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<{ text: string; at: boolean }>()
  const [serverOpen, setServerOpen] = useState(false)
  const [intent, setIntent] = useState<'connecting' | 'disconnecting'>()
  const [subUrl, setSubUrl] = useState('')

  // clod:tun-ready — ошибка запоминается вместе с состоянием подключения, в
  // котором случилась, и перестаёт показываться, как только оно изменилось:
  // поставил службу, включил TUN тумблером — кнопка больше не красная.
  const errorText = failure?.at === connected ? failure.text : undefined

  const state: ConnectState = errorText
    ? 'error'
    : busy
      ? // Намерение фиксируется в момент нажатия: `connected` по ходу дела
        // гаснет (TUN гасится первым, системный прокси следом), и подпись
        // посреди отключения превращалась бы в «Подключение…».
        (intent ?? 'connecting')
      : connected
        ? 'on'
        : 'off'

  const toggle = useLockFn(async () => {
    setIntent(connected ? 'disconnecting' : 'connecting')
    setBusy(true)
    setFailure(undefined)
    try {
      await toggleConnection()
    } catch (error) {
      setFailure({
        text: error instanceof Error ? error.message : String(error),
        at: connected,
      })
    } finally {
      setBusy(false)
      setIntent(undefined)
    }
  })

  const addSubscription = useLockFn(async () => {
    const url = subUrl.trim()
    if (!url) return
    // clod: без enhance ядро не перечитывает конфиг после импорта — список
    // серверов оставался пустым, пока подписку не обновишь руками
    const activate = async () => {
      await mutateProfiles()
      try {
        await enhanceProfiles()
      } catch (error) {
        console.error('[import] enhance after import failed:', error)
      }
    }
    try {
      await importProfile(url)
      await activate()
      setSubUrl('')
    } catch {
      // A URL that is not a subscription is still a perfectly normal mistake;
      // fall back to creating a remote profile so the error names the cause.
      try {
        await createProfile({ type: 'remote', name: url, url })
        await activate()
        setSubUrl('')
      } catch (error) {
        showNotice.error(error)
      }
    }
  })

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
            onKeyDown={(event) => {
              if (event.key === 'Enter') void addSubscription()
            }}
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
        {/* clod:design-v2 — no sidebar: the advanced mode (and with it the
            settings) must stay reachable even before a subscription exists */}
        <Button
          size="small"
          color="inherit"
          sx={{ color: 'text.secondary' }}
          onClick={() => void setSimpleMode(false)}
        >
          {t('home.pages.simple.toAdvanced')}
        </Button>
      </Stack>
    )
  }

  const supportIsTelegram =
    current.support_url?.includes('t.me/') ||
    current.support_url?.startsWith('tg:')

  return (
    <Stack sx={{ height: '100%', overflowY: 'auto' }}>
      {/* One column at every window size: it fills a narrow window and stays
          a readable centred strip on a large display. */}
      {/* clod: gap 1.5, не 2 — после появления строки режима под кнопкой
          контент перестал влезать в simple-окно и вылезала прокрутка */}
      <Stack
        sx={{
          p: 2,
          gap: 1.5,
          width: '100%',
          maxWidth: 520,
          mx: 'auto',
          flex: 1,
        }}
      >
        <ProviderHeader profile={current} />

        <ProviderBanners profile={current} onChanged={mutateProfiles} />

        <Box sx={{ display: 'flex', justifyContent: 'center', pt: 0.5 }}>
          <ConnectButton
            state={state}
            uptime={uptime}
            errorText={errorText}
            onToggle={() => void toggle()}
          />
        </Box>

        {/* clod: и в простом режиме видно, что дёргает Connect (системный
            прокси / TUN / оба) и какой режим маршрутизации активен */}
        <Box sx={{ display: 'flex', justifyContent: 'center', mt: -1 }}>
          <ModeStatus locked={Boolean(current.lock_mode)} />
        </Box>

        {/* clod:tun-ready — в простом режиме карточки быстрых действий нет, а
            объяснить «TUN не поднялся» и дать кнопку починки надо и здесь */}
        <Box sx={{ display: 'flex', justifyContent: 'center' }}>
          <TunStatus />
        </Box>

        {/* clod: session totals (downloaded/uploaded since the core started) */}
        <SessionTraffic />

        <ServerSelectRow onOpen={() => setServerOpen(true)} />
        <ServerSelect open={serverOpen} onClose={() => setServerOpen(false)} />

        <SubscriptionCard profile={current} />

        <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
          {current.portal_url ? (
            <Button
              startIcon={<HomeWorkRoundedIcon />}
              sx={(theme) => ({
                bgcolor: alpha(theme.palette.primary.main, 0.13),
                px: 1.75,
              })}
              onClick={() => void openLink(current.portal_url)}
            >
              {t('home.pages.simple.portal')}
            </Button>
          ) : null}
          {current.support_url ? (
            <Button
              startIcon={
                supportIsTelegram ? (
                  <TelegramIcon />
                ) : (
                  <SupportAgentRoundedIcon />
                )
              }
              sx={(theme) => ({
                bgcolor: theme.palette.action.hover,
                color: theme.palette.text.secondary,
                px: 1.75,
              })}
              onClick={() => void openLink(current.support_url)}
            >
              {t('profiles.components.hwidDialog.support')}
            </Button>
          ) : null}
        </Stack>

        {/* clod:design-v2 — the mockups' footlink into the advanced mode */}
        <Box sx={{ mt: 'auto', textAlign: 'center', pt: 1 }}>
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => void setSimpleMode(false)}
          >
            {t('home.pages.simple.toAdvanced')}
          </Button>
        </Box>
      </Stack>
    </Stack>
  )
}

export default HomeSimplePage
