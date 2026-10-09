import {
  Box,
  Button,
  LinearProgress,
  Stack,
  TextField,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import { CONNECT_RING, ConnectButton } from '@/components/home/connect-button'
import { CoreStatus } from '@/components/home/core-status'
import { FirewallStatus } from '@/components/home/firewall-status'
import { ModeStatus } from '@/components/home/mode-status'
import { ProviderBanners } from '@/components/home/provider-banners'
import { ProviderHeader } from '@/components/home/provider-header'
import { ProviderLinksCard } from '@/components/home/provider-links'
import { ServerSelect, ServerSelectRow } from '@/components/home/server-select'
import { SessionTraffic } from '@/components/home/session-traffic'
import { SubscriptionCard } from '@/components/home/subscription-card'
import { TunStatus } from '@/components/home/tun-status'
import { addStageText, useAddStage } from '@/hooks/use-add-stage'
import { useConnectButton } from '@/hooks/use-connect-button'
import { useProfiles } from '@/hooks/use-profiles'
import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useFitWindowToContent } from '@/hooks/use-window-fit'
import { importProfile } from '@/services/cmds'
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
  const { connected, state, errorText, toggle } = useConnectButton()
  const { simpleMode, setSimpleMode } = useSimpleMode()
  const { fitRef, compact } = useFitWindowToContent()

  const [serverOpen, setServerOpen] = useState(false)
  const [subUrl, setSubUrl] = useState('')
  const [adding, setAdding] = useState(false)
  const { stage: addStage, reset: resetAddStage } = useAddStage()

  const addSubscription = useLockFn(async () => {
    const url = subUrl.trim()
    if (!url) return
    resetAddStage()
    setAdding(true)
    try {
      await importProfile(url)
      await mutateProfiles()
      setSubUrl('')
    } catch (error) {
      showNotice.error(error)
    } finally {
      setAdding(false)
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
              disabled={!subUrl.trim() || adding}
              onClick={() => void addSubscription()}
            >
              {t('shared.actions.add')}
            </Button>
          </Stack>
          {adding && (
            <Box sx={{ px: 0.5 }}>
              <LinearProgress />
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: 'block', mt: 0.5 }}
              >
                {t(
                  ...addStageText(
                    addStage ?? { stage: 'checking', attempt: 1 },
                  ),
                )}
              </Typography>
            </Box>
          )}
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
            onClick={() => setSimpleMode(!simpleMode).catch(showNotice.error)}
          >
            {t(
              simpleMode
                ? 'home.pages.simple.toAdvanced'
                : 'home.pages.advanced.toSimple',
            )}
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

        {/* Кнопка, режим и трафик — одна группа. Сверху место под ореол
            кнопки, чтобы шаг до шапки считался от ореола и кнопка не
            прыгала при подключении. */}
        <Stack sx={{ alignItems: 'center', pt: `${CONNECT_RING}px` }}>
          <ConnectButton
            state={state}
            errorText={errorText}
            compact={compact}
            onToggle={() => void toggle()}
          />
          <Box sx={{ mt: 0.5 }}>
            <ModeStatus
              locked={Boolean(current.lock_mode)}
              permanent={current.lock_permanent === true}
            />
          </Box>
          <Box sx={{ mt: 0.5 }}>
            <SessionTraffic />
          </Box>
        </Stack>

        {/* Без обёрток: пустой блок не должен добавлять шаг. */}
        <CoreStatus />
        <TunStatus />
        <FirewallStatus />

        <ServerSelectRow
          onOpen={() => setServerOpen(true)}
          connected={connected}
        />
        <ServerSelect open={serverOpen} onClose={() => setServerOpen(false)} />

        <SubscriptionCard profile={current} />

        <ProviderLinksCard profile={current} compact={compact} />

        <Box sx={{ mt: 'auto', textAlign: 'center' }}>
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => setSimpleMode(false).catch(showNotice.error)}
          >
            {t('home.pages.simple.toAdvanced')}
          </Button>
        </Box>
      </Stack>
    </Stack>
  )
}

export default HomeSimplePage
