import BoltRoundedIcon from '@mui/icons-material/BoltRounded'
import MinimizeRoundedIcon from '@mui/icons-material/MinimizeRounded'
import PlayCircleRoundedIcon from '@mui/icons-material/PlayCircleRounded'
import PublicRoundedIcon from '@mui/icons-material/PublicRounded'
import SecurityRoundedIcon from '@mui/icons-material/SecurityRounded'
import { alpha, Box, Stack, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Switch } from '@/components/base'
import {
  useConnectTargets,
  useRememberTargets,
} from '@/hooks/use-connect-targets'
import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useSystemState } from '@/hooks/use-system-state'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { CARD_SURFACE, CARD_TITLE, SHAPE, TINT } from '@/pages/_theme'
import { ensureTunReady } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

import { FirewallStatus } from './firewall-status'
import { TunStatus } from './tun-status'

interface RowProps {
  label: string
  icon: ReactNode
  checked: boolean
  disabled?: boolean
  onToggle: (next: boolean) => void
}

const Row = ({ label, icon, checked, disabled, onToggle }: RowProps) => (
  <Stack
    direction="row"
    sx={(theme) => ({
      alignItems: 'center',
      gap: 1,
      minHeight: 34,
      mx: -1,
      px: 1,
      borderRadius: SHAPE.control,
      transition: theme.transitions.create(['background-color'], {
        duration: theme.transitions.duration.short,
      }),
      '&:hover': { bgcolor: 'action.hover' },
    })}
  >
    <Box
      sx={{
        width: 26,
        height: 26,
        borderRadius: SHAPE.chip,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: 'primary.main',
        bgcolor: (theme) => alpha(theme.palette.primary.main, TINT.weak),
        flex: 'none',
        '& svg': { fontSize: 15 },
      }}
    >
      {icon}
    </Box>
    <Typography sx={{ flex: 1, minWidth: 0, fontSize: 13.5 }} noWrap>
      {label}
    </Typography>
    <Switch
      checked={checked}
      disabled={disabled}
      slotProps={{ input: { 'aria-label': label } }}
      onChange={(_event, next) => onToggle(next)}
    />
  </Stack>
)

const GroupCap = ({ label }: { label: string }) => (
  <Typography
    variant="caption"
    sx={{
      fontSize: 10.5,
      fontWeight: 700,
      letterSpacing: '0.8px',
      textTransform: 'uppercase',
      color: 'text.disabled',
      pt: 1,
    }}
  >
    {label}
  </Typography>
)

export const QuickActions = () => {
  const { t } = useTranslation()
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { indicator: sysproxyOn, toggleSystemProxy } = useSystemProxyState()
  const { mutateSystemState } = useSystemState()
  const { targetSys, targetTun, targetsLocked } = useConnectTargets()
  const rememberTarget = useRememberTargets()
  const { tunActive, tunCapable, mutateTunState } = useTunState()
  const [installing, setInstalling] = useState(false)

  const toggleSysproxy = useLockFn(async (next: boolean) => {
    try {
      await toggleSystemProxy(next)
      void rememberTarget('sys', next)
    } catch (error) {
      showNotice.error(error)
    }
  })

  const toggleTun = useLockFn(async (next: boolean) => {
    try {
      if (next && !tunCapable) {
        setInstalling(true)
        const ready = await ensureTunReady().finally(() => setInstalling(false))
        await Promise.all([mutateSystemState(), mutateTunState()])
        if (!ready) {
          showNotice.error(
            'settings.sections.proxyControl.tooltips.tunUnavailable',
          )
          return
        }
      }
      mutateVerge({ ...verge, enable_tun_mode: next }, false)
      await patchVerge({ enable_tun_mode: next })
      void rememberTarget('tun', next)
    } catch (error) {
      showNotice.error(error)
      mutateVerge()
    } finally {
      await mutateTunState()
    }
  })

  const patchFlag = useLockFn(async (patch: Partial<IVergeConfig>) => {
    try {
      await patchVerge(patch)
    } catch (error) {
      showNotice.error(error)
    }
  })

  const autoLaunch = Boolean(verge?.enable_auto_launch)
  const silentStart = Boolean(verge?.enable_silent_start)
  const connectOnLaunch = Boolean(verge?.connect_on_launch)

  return (
    <Stack
      sx={{
        ...CARD_SURFACE,
        alignSelf: 'stretch',
        boxSizing: 'border-box',
        gap: 0.25,
        px: 1.75,
        py: 1.25,
      }}
    >
      <Typography variant="caption" color="text.secondary" sx={CARD_TITLE}>
        {t('home.components.quickActions.title')}
      </Typography>

      <GroupCap label={t('home.components.quickActions.groups.connection')} />
      {targetsLocked ? (
        <Typography variant="caption" color="text.secondary">
          {t('home.components.quickActions.lockedBy', {
            targets: [
              targetSys ? t('home.components.quickActions.sysproxy') : null,
              targetTun ? t('home.components.quickActions.tun') : null,
            ]
              .filter(Boolean)
              .join(' + '),
          })}
        </Typography>
      ) : (
        <>
          <Row
            label={t('home.components.quickActions.sysproxy')}
            icon={<PublicRoundedIcon />}
            checked={sysproxyOn}
            onToggle={(next) => void toggleSysproxy(next)}
          />
          <Row
            label={
              installing
                ? t('home.components.quickActions.tunInstalling')
                : t('home.components.quickActions.tun')
            }
            icon={<SecurityRoundedIcon />}
            checked={tunActive}
            disabled={installing}
            onToggle={(next) => void toggleTun(next)}
          />
          <TunStatus />
        </>
      )}
      <FirewallStatus />

      <GroupCap label={t('home.components.quickActions.groups.launch')} />
      <Row
        label={t('home.components.quickActions.connectOnLaunch')}
        icon={<BoltRoundedIcon />}
        checked={connectOnLaunch}
        onToggle={(next) => void patchFlag({ connect_on_launch: next })}
      />
      <Row
        label={t('home.components.quickActions.autoLaunch')}
        icon={<PlayCircleRoundedIcon />}
        checked={autoLaunch}
        onToggle={(next) => void patchFlag({ enable_auto_launch: next })}
      />
      <Row
        label={t('home.components.quickActions.silentStart')}
        icon={<MinimizeRoundedIcon />}
        checked={silentStart}
        onToggle={(next) => void patchFlag({ enable_silent_start: next })}
      />
    </Stack>
  )
}
