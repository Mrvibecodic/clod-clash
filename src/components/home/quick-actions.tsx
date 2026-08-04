import { Stack, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
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
import { ensureTunReady } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

import { TunStatus } from './tun-status'

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
  // clod: строке нужна собственная высота. Тумблер 26 px при плотной укладке
  // почти касался соседнего — строки читались как наехавшие друг на друга.
  // `size="small"` тут тоже лишний: наш Switch уже компактный, а мелкий размер
  // MUI спорит с его геометрией (свой трек против нашего).
  <Stack direction="row" sx={{ alignItems: 'center', gap: 1, minHeight: 34 }}>
    <Typography sx={{ flex: 1, minWidth: 0, fontSize: 13 }} noWrap>
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
  const { isTunModeAvailable, mutateSystemState } = useSystemState()
  const { targetSys, targetTun } = useConnectTargets()
  // Дёрнутый руками тумблер — это и есть выбор режима: отдельной настройки
  // «что включает Connect» больше нет.
  const rememberTarget = useRememberTargets()
  // Реальное состояние, а не флаг из конфига: тумблер не должен гореть над
  // мёртвым туннелем.
  const { tunActive, mutateTunState } = useTunState()
  // Установка службы идёт в фоне и требует подтверждения прав — тумблер на это
  // время показывает, что происходит, а не замирает.
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
      // clod:tun-ready — TUN нужна фоновая служба. Раньше здесь была ошибка
      // «поставьте службу сами»; теперь пользователь просит TUN — мы её и
      // ставим (один запрос прав), и только отказ оставляет тумблер выключенным.
      if (next && !isTunModeAvailable) {
        setInstalling(true)
        const ready = await ensureTunReady().finally(() => setInstalling(false))
        await mutateSystemState()
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
      // Оптимистичное значение выше могло разойтись с бэкендом (там патч
      // откатывается через discard), поэтому перечитываем конфиг.
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

  // The same values the settings page writes — one source, not a second copy.
  const autoLaunch = Boolean(verge?.enable_auto_launch)
  const silentStart = Boolean(verge?.enable_silent_start)

  return (
    <Stack
      sx={{
        alignSelf: 'stretch',
        boxSizing: 'border-box',
        gap: 0.25,
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
            label={
              installing
                ? t('home.components.quickActions.tunInstalling')
                : t('home.components.quickActions.tun')
            }
            checked={tunActive}
            disabled={installing}
            onToggle={(next) => void toggleTun(next)}
          />
          <TunStatus />
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
