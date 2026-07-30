import DescriptionRoundedIcon from '@mui/icons-material/DescriptionRounded'
import HomeWorkRoundedIcon from '@mui/icons-material/HomeWorkRounded'
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import SettingsRoundedIcon from '@mui/icons-material/SettingsRounded'
import SpeedOutlinedIcon from '@mui/icons-material/SpeedOutlined'
import SupportAgentRoundedIcon from '@mui/icons-material/SupportAgentRounded'
import TerminalRoundedIcon from '@mui/icons-material/TerminalRounded'
import { Box, Button, ButtonBase, Stack, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import {
  ConnectButton,
  type ConnectState,
} from '@/components/home/connect-button'
import { EnhancedCard } from '@/components/home/enhanced-card'
import { EnhancedTrafficStats } from '@/components/home/enhanced-traffic-stats'
import { ProviderBanners } from '@/components/home/provider-banners'
import { ProviderHeader } from '@/components/home/provider-header'
import { ServerSelect, ServerSelectRow } from '@/components/home/server-select'
import { SubscriptionCard } from '@/components/home/subscription-card'
import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useProfiles } from '@/hooks/use-profiles'
import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useClashConfigData, useUptimeData } from '@/providers/app-data-context'
import { openWebUrl, updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

import HomeSimplePage from './home-simple'

interface TileProps {
  icon: ReactNode
  label: string
  hint?: string
  onClick: () => void
}

/** One quick-access tile of the advanced home screen. */
const Tile = ({ icon, label, hint, onClick }: TileProps) => (
  <ButtonBase
    onClick={onClick}
    sx={{
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'flex-start',
      gap: 1.5,
      p: 1.75,
      borderRadius: 2,
      textAlign: 'left',
      border: (theme) => `1px solid ${theme.palette.divider}`,
      '&:hover': { borderColor: 'primary.main', bgcolor: 'action.hover' },
    }}
  >
    <Box
      sx={{
        width: 36,
        height: 36,
        borderRadius: 1.5,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: 'primary.main',
        bgcolor: 'action.hover',
        flex: 'none',
      }}
    >
      {icon}
    </Box>
    <Box sx={{ minWidth: 0 }}>
      <Typography variant="body2" sx={{ fontWeight: 600 }} noWrap>
        {label}
      </Typography>
      {hint ? (
        <Typography variant="caption" color="text.secondary" noWrap>
          {hint}
        </Typography>
      ) : null}
    </Box>
  </ButtonBase>
)

/**
 * The advanced interface: still one screen, just a wider one.
 *
 * Left — the connection zone (button, active modes, server). Right — the
 * subscription, live traffic and quick tiles into the deeper sections. On a
 * narrow window the columns stack, which is also the layout a future mobile
 * build starts from. Proxy and routing modes are read-only here: changing
 * them lives in the settings, and `clod-lock-mode` removes even that.
 */
const HomeAdvancedPage = () => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { current, mutateProfiles } = useProfiles()
  const { uptime } = useUptimeData()
  const { clashConfig } = useClashConfigData()
  const { setSimpleMode } = useSimpleMode()
  const { connected, targetSys, targetTun, toggleConnection } =
    useConnectTargets()

  const [busy, setBusy] = useState(false)
  const [errorText, setErrorText] = useState<string>()
  const [serverOpen, setServerOpen] = useState(false)

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
      await toggleConnection()
    } catch (error) {
      setErrorText(error instanceof Error ? error.message : String(error))
    } finally {
      setBusy(false)
    }
  })

  const refreshSubscription = useLockFn(async () => {
    if (!current?.uid) return
    try {
      await updateProfile(current.uid)
      await mutateProfiles()
      showNotice.success('profiles.feedback.notifications.updateSuccess')
    } catch (error) {
      showNotice.error(error)
    }
  })

  const openLink = useLockFn(async (url?: string) => {
    if (!url) return
    try {
      await openWebUrl(url)
    } catch (error) {
      showNotice.error(error)
    }
  })

  // Without a subscription the advanced screen has nothing extra to offer;
  // the simple welcome (paste a link) is the right screen in both modes.
  if (!current) {
    return <HomeSimplePage />
  }

  const clashMode = clashConfig?.mode?.toLowerCase()
  const modeLabel =
    clashMode === 'global' || clashMode === 'direct' || clashMode === 'rule'
      ? t(`home.components.clashMode.labels.${clashMode}`)
      : t('home.components.clashMode.labels.rule')

  const activeTargets = [
    targetSys ? t('settings.components.verge.basic.options.sysproxy') : null,
    targetTun ? t('settings.components.verge.basic.options.tun') : null,
  ]
    .filter(Boolean)
    .join(' + ')

  return (
    <Stack sx={{ height: '100%', overflowY: 'auto' }}>
      <Box sx={{ px: 2, pt: 2 }}>
        <ProviderHeader profile={current} />
      </Box>

      <Box
        sx={{
          display: 'flex',
          flexDirection: { xs: 'column', md: 'row' },
          flex: 1,
          minHeight: 0,
          mt: 1,
        }}
      >
        {/* connection zone */}
        <Stack
          sx={{
            width: { md: 320 },
            flex: 'none',
            alignItems: 'center',
            gap: 1.5,
            px: 2,
            py: 2,
            borderRight: { md: 1 },
            borderColor: { md: 'divider' },
          }}
        >
          <ConnectButton
            state={state}
            uptime={uptime}
            errorText={errorText}
            onToggle={() => void toggle()}
          />

          {/* Which switches Connect drives and the routing mode. Read-only on
              purpose: the mode lives in the settings, or nowhere at all when
              the panel locked it. */}
          {current.lock_mode ? (
            <Typography variant="caption" color="text.secondary">
              {activeTargets} · {modeLabel}
            </Typography>
          ) : (
            <Button
              size="small"
              color="inherit"
              sx={{ color: 'text.secondary', textTransform: 'none' }}
              onClick={() => void navigate('/settings')}
            >
              {activeTargets} · {modeLabel}
            </Button>
          )}

          <ServerSelectRow onOpen={() => setServerOpen(true)} />
          <ServerSelect
            open={serverOpen}
            onClose={() => setServerOpen(false)}
          />

          <Box sx={{ flex: 1 }} />
          <Button
            size="small"
            color="inherit"
            sx={{ color: 'text.secondary' }}
            onClick={() => setSimpleMode(true)}
          >
            {t('home.pages.advanced.toSimple')}
          </Button>
        </Stack>

        {/* subscription, traffic, quick access */}
        <Stack
          sx={{ flex: 1, minWidth: 0, gap: 1.5, p: 2, pt: { xs: 0, md: 2 } }}
        >
          <ProviderBanners profile={current} onChanged={mutateProfiles} />

          <SubscriptionCard profile={current} />

          <EnhancedCard
            title={t('home.page.cards.trafficStats')}
            icon={<SpeedOutlinedIcon />}
            iconColor="secondary"
          >
            <EnhancedTrafficStats />
          </EnhancedCard>

          <Box
            sx={{
              display: 'grid',
              gridTemplateColumns: { xs: '1fr 1fr', lg: '1fr 1fr 1fr' },
              gap: 1.25,
            }}
          >
            {current.portal_url ? (
              <Tile
                icon={<HomeWorkRoundedIcon fontSize="small" />}
                label={t('home.pages.simple.portal')}
                hint={t('home.pages.advanced.tiles.portalHint')}
                onClick={() => void openLink(current.portal_url)}
              />
            ) : null}
            {current.support_url ? (
              <Tile
                icon={<SupportAgentRoundedIcon fontSize="small" />}
                label={t('profiles.components.hwidDialog.support')}
                onClick={() => void openLink(current.support_url)}
              />
            ) : null}
            <Tile
              icon={<DescriptionRoundedIcon fontSize="small" />}
              label={t('layout.components.navigation.tabs.profiles')}
              onClick={() => void navigate('/profile')}
            />
            <Tile
              icon={<TerminalRoundedIcon fontSize="small" />}
              label={t('layout.components.navigation.tabs.logs')}
              onClick={() => void navigate('/logs')}
            />
            <Tile
              icon={<RefreshRoundedIcon fontSize="small" />}
              label={t('home.pages.advanced.tiles.refresh')}
              onClick={() => void refreshSubscription()}
            />
            <Tile
              icon={<SettingsRoundedIcon fontSize="small" />}
              label={t('layout.components.navigation.tabs.settings')}
              onClick={() => void navigate('/settings')}
            />
          </Box>
        </Stack>
      </Box>
    </Stack>
  )
}

export default HomeAdvancedPage
