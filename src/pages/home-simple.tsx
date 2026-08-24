import {
  Box,
  Button,
  Checkbox,
  FormControlLabel,
  Stack,
  TextField,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import {
  ConnectButton,
  type ConnectState,
} from '@/components/home/connect-button'
import { FirewallStatus } from '@/components/home/firewall-status'
import { ModeStatus } from '@/components/home/mode-status'
import { ProviderBanners } from '@/components/home/provider-banners'
import { ProviderHeader } from '@/components/home/provider-header'
import { ProviderLinksCard } from '@/components/home/provider-links'
import { ServerSelect, ServerSelectRow } from '@/components/home/server-select'
import { SessionTraffic } from '@/components/home/session-traffic'
import { SubscriptionCard } from '@/components/home/subscription-card'
import { TunStatus } from '@/components/home/tun-status'
import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useProfiles } from '@/hooks/use-profiles'
import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useFitWindowToContent } from '@/hooks/use-window-fit'
import { createProfile, enhanceProfiles, importProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

const HomeSimplePage = () => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const {
    current,
    profiles,
    error: profilesError,
    mutateProfiles,
  } = useProfiles()
  const { connected, willConnect, toggleConnection } = useConnectTargets()
  const { setSimpleMode } = useSimpleMode()
  const { fitRef, compact } = useFitWindowToContent()

  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<{ text: string; at: boolean }>()
  const [serverOpen, setServerOpen] = useState(false)
  const [intent, setIntent] = useState<'connecting' | 'disconnecting'>()
  const [subUrl, setSubUrl] = useState('')
  const [subSecure, setSubSecure] = useState(false)

  const errorText = failure?.at === connected ? failure.text : undefined

  const state: ConnectState = errorText
    ? 'error'
    : busy
      ? (intent ?? 'connecting')
      : connected
        ? 'on'
        : 'off'

  const toggle = useLockFn(async () => {
    setIntent(willConnect ? 'connecting' : 'disconnecting')
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
    const activate = async () => {
      await mutateProfiles()
      try {
        await enhanceProfiles()
      } catch (error) {
        console.error('[import] enhance after import failed:', error)
      }
    }
    const option = subSecure ? { with_proxy: true, secure: true } : undefined
    try {
      await importProfile(url, option)
      await activate()
      setSubUrl('')
    } catch {
      try {
        await createProfile({ type: 'remote', url, option })
        await activate()
        setSubUrl('')
      } catch (error) {
        showNotice.error(error)
      }
    }
  })

  if (!profiles && !profilesError) {
    return <Stack sx={{ height: '100%' }} />
  }

  if (!current) {
    return (
      <Stack
        ref={fitRef}
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
        <Stack sx={{ gap: 0.5, width: '100%', maxWidth: 480 }}>
          <Stack direction="row" sx={{ gap: 1 }}>
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
          <FormControlLabel
            sx={{ ml: 0.5, mr: 0 }}
            control={
              <Checkbox
                size="small"
                checked={subSecure}
                onChange={(event) => setSubSecure(event.target.checked)}
              />
            }
            label={
              <Typography variant="body2" color="text.secondary">
                {t('profiles.modals.profileForm.fields.secureChannel')}
              </Typography>
            }
          />
        </Stack>
        <Stack direction="row" sx={{ gap: 1 }}>
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => void navigate('/settings')}
          >
            {t('layout.components.navigation.tabs.settings')}
          </Button>
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => void setSimpleMode(false)}
          >
            {t('home.pages.simple.toAdvanced')}
          </Button>
        </Stack>
      </Stack>
    )
  }

  return (
    <Stack ref={fitRef} sx={{ height: '100%', overflowY: 'auto' }}>
      <Stack
        sx={{
          p: compact ? 1.25 : 2,
          gap: compact ? 1 : 1.5,
          width: '100%',
          maxWidth: 520,
          mx: 'auto',
          flex: '1 0 auto',
        }}
      >
        <ProviderHeader profile={current} showSettings />

        <ProviderBanners profile={current} onChanged={mutateProfiles} />

        <Box
          sx={{
            display: 'flex',
            justifyContent: 'center',
            pt: compact ? 0 : 0.5,
          }}
        >
          <ConnectButton
            state={state}
            errorText={errorText}
            compact={compact}
            onToggle={() => void toggle()}
          />
        </Box>

        <Box sx={{ display: 'flex', justifyContent: 'center', mt: -1 }}>
          <ModeStatus locked={Boolean(current.lock_mode)} />
        </Box>

        <Box sx={{ display: 'flex', justifyContent: 'center' }}>
          <TunStatus />
        </Box>

        <Box sx={{ display: 'flex', justifyContent: 'center' }}>
          <FirewallStatus />
        </Box>

        <SessionTraffic />

        <ServerSelectRow onOpen={() => setServerOpen(true)} />
        <ServerSelect open={serverOpen} onClose={() => setServerOpen(false)} />

        <SubscriptionCard profile={current} />

        <ProviderLinksCard profile={current} compact={compact} />

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
