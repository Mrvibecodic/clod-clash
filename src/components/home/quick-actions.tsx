import { Stack, Switch, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useTranslation } from 'react-i18next'

import { useConnectTargets } from '@/hooks/use-connect-targets'
import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useSystemState } from '@/hooks/use-system-state'
import { useVerge } from '@/hooks/use-verge'
import { showNotice } from '@/services/notice-service'

interface Props {
  /** `clod-lock-mode`: the panel forbids changing how the app connects. */
  locked?: boolean
}

interface RowProps {
  label: string
  checked: boolean
  disabled?: boolean
  onToggle: (next: boolean) => void
}

const Row = ({ label, checked, disabled, onToggle }: RowProps) => (
  <Stack direction="row" sx={{ alignItems: 'center', gap: 1 }}>
    <Typography sx={{ flex: 1, minWidth: 0, fontSize: 13 }} noWrap>
      {label}
    </Typography>
    <Switch
      size="small"
      checked={checked}
      disabled={disabled}
      slotProps={{ input: { 'aria-label': label } }}
      onChange={(_event, next) => onToggle(next)}
    />
  </Stack>
)

/**
 * clod: the advanced screen's left column used to end at the server row and
 * leave the rest of the height empty. These are the four switches worth
 * reaching for without opening the settings.
 *
 * The two connection switches drive the *live* state — the system proxy is
 * turned on right now, TUN is turned on right now — unlike the settings pair
 * («Подключение: …»), which only decides what the Connect button drives. Two
 * different questions, so they stay two different controls; the labels here
 * carry no «Подключение:» prefix for exactly that reason.
 */
export const QuickActions = ({ locked }: Props) => {
  const { t } = useTranslation()
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { indicator: sysproxyOn, toggleSystemProxy } = useSystemProxyState()
  const { isTunModeAvailable } = useSystemState()
  const { targetSys, targetTun } = useConnectTargets()

  const tunOn = Boolean(verge?.enable_tun_mode)

  const toggleSysproxy = useLockFn(async (next: boolean) => {
    try {
      await toggleSystemProxy(next)
    } catch (error) {
      showNotice.error(error)
    }
  })

  const toggleTun = useLockFn(async (next: boolean) => {
    // TUN needs the helper service; without it the core would fail to start
    // the tunnel and the switch would silently bounce back.
    if (next && !isTunModeAvailable) {
      showNotice.error('settings.sections.proxyControl.tooltips.tunUnavailable')
      return
    }
    try {
      mutateVerge({ ...verge, enable_tun_mode: next }, false)
      await patchVerge({ enable_tun_mode: next })
    } catch (error) {
      showNotice.error(error)
    }
  })

  const patchFlag = useLockFn(async (patch: Partial<IVergeConfig>) => {
    try {
      await patchVerge(patch)
    } catch (error) {
      showNotice.error(error)
    }
  })

  // The same values the settings page writes — one source, not a second copy.
  const autoLaunch = Boolean(verge?.enable_auto_launch)
  const silentStart = Boolean(verge?.enable_silent_start)

  return (
    <Stack
      sx={{
        alignSelf: 'stretch',
        boxSizing: 'border-box',
        gap: 0.5,
        px: 1.75,
        py: 1.25,
        borderRadius: '14px',
        bgcolor: 'background.paper',
        border: (theme) => `1px solid ${theme.palette.divider}`,
      }}
    >
      <Typography variant="caption" color="text.secondary">
        {t('home.components.quickActions.title')}
      </Typography>

      {/* clod-lock-mode: the provider decided how this client connects, so the
          two connection switches are gone — not merely disabled, which one
          click in the devtools would undo. The state itself stays visible. */}
      {locked ? (
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
            checked={sysproxyOn}
            onToggle={(next) => void toggleSysproxy(next)}
          />
          <Row
            label={t('home.components.quickActions.tun')}
            checked={tunOn}
            disabled={!isTunModeAvailable && !tunOn}
            onToggle={(next) => void toggleTun(next)}
          />
        </>
      )}

      <Row
        label={t('home.components.quickActions.autoLaunch')}
        checked={autoLaunch}
        onToggle={(next) => void patchFlag({ enable_auto_launch: next })}
      />
      <Row
        label={t('home.components.quickActions.silentStart')}
        checked={silentStart}
        onToggle={(next) => void patchFlag({ enable_silent_start: next })}
      />
    </Stack>
  )
}
