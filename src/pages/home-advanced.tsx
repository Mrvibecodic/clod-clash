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
import { useFitWindowToContent } from '@/hooks/use-window-fit'
import { updateProfile } from '@/services/cmds'
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
      p: 1.6,
      borderRadius: '14px',
      textAlign: 'left',
      bgcolor: 'background.paper',
      border: (theme) => `1px solid ${theme.palette.divider}`,
      '&:hover': { borderColor: 'primary.main', bgcolor: 'action.hover' },
    }}
  >
    <Box
      sx={{
        width: 36,
        height: 36,
        borderRadius: '10px',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        color: 'primary.main',
        bgcolor: (theme) => alpha(theme.palette.primary.main, 0.13),
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
  const { setSimpleMode } = useSimpleMode()
  const { connected, willConnect, toggleConnection } = useConnectTargets()
  const uptime = useSessionUptime(connected)
  // clod:fit-window — окно тянется под содержимое, а на маленьком экране
  // сначала поджимается вёрстка и только потом появляется прокрутка
  const { fitRef, compact } = useFitWindowToContent()

  const [busy, setBusy] = useState(false)
  const [failure, setFailure] = useState<{ text: string; at: boolean }>()
  const [serverOpen, setServerOpen] = useState(false)
  const [intent, setIntent] = useState<'connecting' | 'disconnecting'>()

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
    // Подпись — от того, что нажатие СДЕЛАЕТ. Это почти всегда обратное
    // показанному, но подавленный туннель оставляет кнопку тёмной, а нажатие
    // при поднятом системном прокси всё равно отключает.
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

  // Without a subscription the advanced screen has nothing extra to offer;
  // the simple welcome (paste a link) is the right screen in both modes.
  if (!current) {
    return <HomeSimplePage />
  }

  return (
    // clod:fit-window — окно садится по высоте этого содержимого; ref нужен
    // именно на прокручиваемом корне: его `scrollHeight` и есть полная высота.
    <Stack ref={fitRef} sx={{ height: '100%', overflowY: 'auto' }}>
      {/* flexShrink: 0 — иначе при нехватке высоты весь дефицит уходит в
          единственного сжимаемого соседа, и шапка провайдера сплющивается
          вместо того, чтобы страница прокрутилась. */}
      <Box
        sx={{
          px: compact ? 1.25 : 2,
          pt: compact ? 1.25 : 2,
          flexShrink: 0,
        }}
      >
        <ProviderHeader profile={current} />
      </Box>

      {/* clod: было `flex: 1` c `minHeight: 0` — ряд жёстко упирался в высоту
          окна, и лишнее содержимое не прокручивалось, а СЖИМАЛО колонки:
          карточка «Сеть» схлопывалась до заголовка, нижняя плитка обрезалась.
          `1 0 auto` = расти до окна, но никогда не ниже своего содержимого;
          дальше прокручивается внешний Stack, у него `overflowY: auto`. */}
      <Box
        sx={{
          display: 'flex',
          flexDirection: { xs: 'column', md: 'row' },
          flex: '1 0 auto',
          mt: 1,
        }}
      >
        {/* connection zone */}
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

          {/* Which switches Connect drives and the routing mode. Read-only on
              purpose: the mode lives in the settings, or nowhere at all when
              the panel locked it. */}
          <ModeStatus locked={Boolean(current.lock_mode)} showTargets={false} />

          <ServerSelectRow onOpen={() => setServerOpen(true)} />
          <ServerSelect
            open={serverOpen}
            onClose={() => setServerOpen(false)}
          />

          {/* clod: колонка кончалась строкой сервера и дальше пустовала до
              самого низа — четыре переключателя, за которыми иначе лезут в
              настройки, закрывают её и ничего не растягивают. */}
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

        {/* subscription, traffic, quick access */}
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

          {/* clod:design-v2 — the mockups' compact «Network» card */}
          <NetCard />

          {/* clod: столбцов столько, сколько влезает по 190px, а не по точке
              излома `lg`: она считает ШИРИНУ ОКНА, а плитки живут в правой
              колонке — на 1100px оставалась двухстолбцовая сетка в три ряда,
              хотя места хватало на три столбца в два ряда. */}
          {/* clod:provider-links — ссылки провайдера отдельной строкой: пятью
              плитками они забивали бы сетку и переставали отличаться от
              кнопок самого приложения. */}
          <ProviderLinksCard profile={current} compact={compact} />

          <Box
            sx={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(190px, 1fr))',
              gap: compact ? 1 : 1.25,
            }}
          >
            {/* clod:design-v2 — the home screen keeps the everyday tiles
                only; the technical sections (proxies, rules, connections,
                logs) moved into the settings to leave room for the coming
                account button */}
            <Tile
              icon={<DescriptionRoundedIcon fontSize="small" />}
              label={t('home.pages.advanced.tiles.subscriptions')}
              hint={t('home.pages.advanced.tiles.profilesHint')}
              onClick={() => void navigate('/profile')}
            />
            <Tile
              icon={<RefreshRoundedIcon fontSize="small" />}
              label={t('home.pages.advanced.tiles.refresh')}
              onClick={() => void refreshSubscription()}
            />
            <Tile
              icon={<SettingsRoundedIcon fontSize="small" />}
              label={t('layout.components.navigation.tabs.settings')}
              hint={t('home.pages.advanced.tiles.settingsHint')}
              onClick={() => void navigate('/settings')}
            />
          </Box>
        </Stack>
      </Box>
    </Stack>
  )
}

export default HomeAdvancedPage
