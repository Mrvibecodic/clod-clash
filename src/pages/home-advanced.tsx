import DescriptionRoundedIcon from '@mui/icons-material/DescriptionRounded'
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import SettingsRoundedIcon from '@mui/icons-material/SettingsRounded'
import {
  alpha,
  Box,
  Button,
  ButtonBase,
  Stack,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

import {
  ConnectButton,
  type ConnectState,
} from '@/components/home/connect-button'
import { ModeStatus } from '@/components/home/mode-status'
import { NetCard } from '@/components/home/net-card'
import { ProviderBanners } from '@/components/home/provider-banners'
import { ProviderHeader } from '@/components/home/provider-header'
import { ProviderLinksCard } from '@/components/home/provider-links'
import { QuickActions } from '@/components/home/quick-actions'
import { ServerSelect, ServerSelectRow } from '@/components/home/server-select'
import { SubscriptionCard } from '@/components/home/subscription-card'
import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useProfiles } from '@/hooks/use-profiles'
import { useSessionUptime } from '@/hooks/use-session-uptime'
import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useToolShortcuts } from '@/hooks/use-tool-shortcuts'
import { useFitWindowToContent } from '@/hooks/use-window-fit'
import { CARD_SURFACE, SHAPE, TINT } from '@/pages/_theme'
import { updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

import HomeSimplePage from './home-simple'

interface TileProps {
  icon: ReactNode
  label: string
  hint?: string
  onClick: () => void
  dense?: boolean
}

const Tile = ({ icon, label, hint, onClick, dense }: TileProps) => (
  <ButtonBase
    onClick={onClick}
    sx={{
      ...CARD_SURFACE,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'flex-start',
      gap: dense ? 1.25 : 1.5,
      p: dense ? 1.15 : 1.6,
      textAlign: 'left',
      transition: (theme) =>
        theme.transitions.create(
          ['border-color', 'background-color', 'transform', 'box-shadow'],
          { duration: theme.transitions.duration.short },
        ),
      '&:hover': {
        borderColor: 'primary.main',
        bgcolor: 'action.hover',
        transform: 'translateY(-2px)',
        boxShadow: 'var(--card-shadow-hover)',
      },
      '&:active': { transform: 'none' },
      '@media (prefers-reduced-motion: reduce)': {
        '&:hover': { transform: 'none' },
      },
    }}
  >
    <Box
      sx={{
        width: dense ? 30 : 36,
        height: dense ? 30 : 36,
        borderRadius: SHAPE.control,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: 'primary.main',
        bgcolor: (theme) => alpha(theme.palette.primary.main, TINT.base),
        flex: 'none',
      }}
    >
      {icon}
    </Box>
    <Box sx={{ minWidth: 0 }}>
      <Typography
        variant="body2"
        sx={{ fontWeight: 600, fontSize: dense ? 13.5 : undefined }}
        noWrap
      >
        {label}
      </Typography>
      {hint ? (
        <Typography
          variant="caption"
          color="text.secondary"
          noWrap
          sx={{ display: 'block' }}
        >
          {hint}
        </Typography>
      ) : null}
    </Box>
  </ButtonBase>
)

const HomeAdvancedPage = () => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const { current, mutateProfiles } = useProfiles()
  const { setSimpleMode } = useSimpleMode()
  const { shortcuts } = useToolShortcuts()
  const { connected, willConnect, toggleConnection } = useConnectTargets()
  const uptime = useSessionUptime(connected)
  const { fitRef, compact } = useFitWindowToContent()

  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<{ text: string; at: boolean }>()
  const [serverOpen, setServerOpen] = useState(false)
  const [intent, setIntent] = useState<'connecting' | 'disconnecting'>()

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

  const refreshSubscription = useLockFn(async () => {
    if (!current?.uid) return
    try {
      await updateProfile(current.uid)
      await mutateProfiles()
      showNotice.success('home.components.subscription.updated')
    } catch (error) {
      showNotice.error(error)
    }
  })

  if (!current) {
    return <HomeSimplePage />
  }

  const refreshedHint = current.updated
    ? t('home.pages.advanced.tiles.refreshHint', {
        time: dayjs(current.updated * 1000).format('DD.MM · HH:mm'),
      })
    : undefined

  return (
    <Stack ref={fitRef} sx={{ height: '100%', overflowY: 'auto' }}>
      <Box
        sx={{
          px: compact ? 1.25 : 2,
          pt: compact ? 1.25 : 2,
          flexShrink: 0,
        }}
      >
        <ProviderHeader profile={current} />
      </Box>

      <Box
        sx={{
          display: 'flex',
          flexDirection: { xs: 'column', md: 'row' },
          flex: '1 0 auto',
          mt: 1,
        }}
      >
        <Stack
          sx={{
            width: { md: 320 },
            flex: 'none',
            alignItems: 'center',
            gap: compact ? 1 : 1.5,
            px: compact ? 1.25 : 2,
            py: compact ? 1.25 : 2,
            borderRight: { md: 1 },
            borderColor: { md: 'divider' },
          }}
        >
          <ConnectButton
            state={state}
            uptime={uptime}
            errorText={errorText}
            compact={compact}
            onToggle={() => void toggle()}
          />

          <ModeStatus locked={Boolean(current.lock_mode)} showTargets={false} />

          <ServerSelectRow onOpen={() => setServerOpen(true)} />
          <ServerSelect
            open={serverOpen}
            onClose={() => setServerOpen(false)}
          />

          <QuickActions />

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

        <Stack
          sx={{
            flex: 1,
            minWidth: 0,
            gap: compact ? 1 : 1.5,
            p: compact ? 1.25 : 2,
            pt: { xs: 0, md: compact ? 1.25 : 2 },
          }}
        >
          <ProviderBanners profile={current} onChanged={mutateProfiles} />

          <SubscriptionCard profile={current} />

          <NetCard />

          <ProviderLinksCard profile={current} compact={compact} />

          <Box
            sx={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(190px, 1fr))',
              gap: compact ? 1 : 1.25,
            }}
          >
            <Tile
              icon={<DescriptionRoundedIcon fontSize="small" />}
              label={t('home.pages.advanced.tiles.subscriptions')}
              hint={t('home.pages.advanced.tiles.profilesHint')}
              onClick={() => void navigate('/profile')}
            />
            <Tile
              icon={<RefreshRoundedIcon fontSize="small" />}
              label={t('home.pages.advanced.tiles.refresh')}
              hint={refreshedHint}
              onClick={() => void refreshSubscription()}
            />
            <Tile
              icon={<SettingsRoundedIcon fontSize="small" />}
              label={t('layout.components.navigation.tabs.settings')}
              hint={t('home.pages.advanced.tiles.settingsHint')}
              onClick={() => void navigate('/settings')}
            />
          </Box>

          {shortcuts.length > 0 ? (
            <Box
              sx={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))',
                gap: compact ? 1 : 1.25,
              }}
            >
              {shortcuts.map((tool) => (
                <Tile
                  key={tool.key}
                  dense
                  icon={<tool.Icon fontSize="small" />}
                  label={t(tool.label)}
                  onClick={() => void navigate(tool.path)}
                />
              ))}
            </Box>
          ) : null}
        </Stack>
      </Box>
    </Stack>
  )
}

export default HomeAdvancedPage
