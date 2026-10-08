import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import SettingsRoundedIcon from '@mui/icons-material/SettingsRounded'
import {
  alpha,
  Badge,
  Box,
  CircularProgress,
  IconButton,
  Stack,
  Typography,
} from '@mui/material'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import useSWR from 'swr'

import { useExpiryCountdown } from '@/hooks/use-expiry-countdown'
import { useSubscriptionUpdate } from '@/hooks/use-subscription-update'
import { getProfileLogo } from '@/services/cmds'
import { profileDisplayName } from '@/utils/profile-name'
import { clockSkew, missedUpdates } from '@/utils/subscription-status'

interface Props {
  profile: IProfileItem
  /**
   * clod:settings-entry — показать шестерёнку рядом с обновлением.
   *
   * Нужна простому режиму: боковой колонки в клиенте нет, а на простом экране
   * плиток тоже нет — после её удаления попасть в настройки можно было только
   * через серую строку режима под кнопкой Connect, да и та при `clod-lock-mode`
   * превращается в неактивный текст, то есть выход в настройки пропадал вовсе.
   * На расширенном экране плитка «Настройки» есть, и второй вход там лишний.
   */
  showSettings?: boolean
}

/**
 * Provider identity row, 1:1 with the mockups: a 42 px rounded logo (the
 * provider's image, or the first letter on an accent gradient), the plan
 * name in bold with the subscription state underneath, and the refresh
 * button on the right.
 */
export const ProviderHeader = ({ profile, showSettings }: Props) => {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const {
    updating: refreshing,
    paused,
    refresh,
  } = useSubscriptionUpdate(profile.uid)

  // clod: логотип берём из локального кэша, а не с чужого хоста: он не мигает
  // при старте, работает офлайн и не отдаёт IP пользователя при каждом показе.
  // Ключ — только uid, и это принципиально: команда отдаёт `data:`-URL с
  // картинкой (до 2 МиБ), а кэш SWR глобальный и без вытеснения. Время
  // обновления подписки в ключе означало бы новую запись с новой копией
  // картинки на КАЖДОЕ обновление, и ни одна из них не освобождается — клиент
  // работает сутками. За свежесть отвечает бэкенд: `logo_cache::sync` после
  // обновления подписки перепроверяет картинку и, если она сменилась, шлёт
  // `clod://profile-picture` — по нему эта же запись перечитывается
  // (`use-layout-events`), когда новая картинка уже на диске. Ревалидация
  // заменяет одну запись, а не добавляет новую, поэтому память не растёт.
  const { data: cachedLogo, isLoading: logoLoading } = useSWR(
    profile.uid ? ['profileLogo', profile.uid] : null,
    ([, uid]) => getProfileLogo(uid as string),
    { revalidateOnFocus: false },
  )

  // Пока кэш читается — не показываем ничего: подставить сюда URL из заголовка
  // значило бы сходить на хост провайдера ровно в тот момент, которого мы и
  // хотели избежать. URL остаётся фолбэком только когда кэша нет совсем.
  const logo = logoLoading ? undefined : (cachedLogo ?? profile.logo)

  const expire = profile.extra?.expire ?? 0
  const countdown = useExpiryCountdown(expire, clockSkew(profile) ?? 0)
  const expired = expire > 0 && countdown.secondsLeft <= 0
  // clod: два автообновления подряд не прошли — кнопка в янтарной обводке с точкой
  const stale = !refreshing && missedUpdates(profile) >= 2
  const refreshLabel = stale
    ? t('profiles.components.profileItem.status.staleTitle')
    : t('shared.actions.refresh')

  return (
    <Stack direction="row" sx={{ alignItems: 'center', gap: 1.5 }}>
      {/* clod: плитка — это лого бренда провайдера; нет лого — нет плитки.
          Буквенный фолбэк спотыкался об эмодзи в имени (пол-суррогата → «?») */}
      {logo ? (
        <Box
          component="img"
          src={logo}
          alt=""
          sx={{
            width: 42,
            height: 42,
            borderRadius: '12px',
            objectFit: 'cover',
            flex: 'none',
          }}
        />
      ) : null}
      <Box sx={{ flex: 1, minWidth: 0 }}>
        <Typography noWrap sx={{ fontSize: 15, fontWeight: 700 }}>
          {profileDisplayName(profile)}
        </Typography>
        <Typography
          noWrap
          sx={{ fontSize: 12 }}
          color={expired ? 'error' : 'text.secondary'}
        >
          {expired
            ? t('home.components.providerHeader.expired')
            : t('home.components.providerHeader.active')}
        </Typography>
      </Box>
      <IconButton
        onClick={() => void refresh()}
        disabled={refreshing || paused}
        aria-label={refreshLabel}
        title={stale ? refreshLabel : undefined}
        sx={[
          { borderRadius: '10px' },
          stale &&
            ((theme) => ({
              color: 'warning.main',
              border: `2px solid ${theme.palette.warning.main}`,
              bgcolor: alpha(theme.palette.warning.main, 0.14),
              '&:hover': { bgcolor: alpha(theme.palette.warning.main, 0.22) },
            })),
        ]}
      >
        {refreshing ? (
          <CircularProgress size={20} />
        ) : (
          <Badge variant="dot" color="warning" invisible={!stale}>
            <RefreshRoundedIcon />
          </Badge>
        )}
      </IconButton>
      {showSettings ? (
        <IconButton
          onClick={() => void navigate('/settings')}
          aria-label={t('layout.components.navigation.tabs.settings')}
          title={t('layout.components.navigation.tabs.settings')}
          sx={{ borderRadius: '10px' }}
        >
          <SettingsRoundedIcon />
        </IconButton>
      ) : null}
    </Stack>
  )
}
