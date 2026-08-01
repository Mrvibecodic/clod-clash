import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import {
  Box,
  CircularProgress,
  IconButton,
  Stack,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

import { useProfiles } from '@/hooks/use-profiles'
import { getProfileLogo, updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { toUnixSeconds } from '@/utils/subscription-status'

interface Props {
  profile: IProfileItem
}

/**
 * Provider identity row, 1:1 with the mockups: a 42 px rounded logo (the
 * provider's image, or the first letter on an accent gradient), the plan
 * name in bold with the subscription state underneath, and the refresh
 * button on the right.
 */
export const ProviderHeader = ({ profile }: Props) => {
  const { t } = useTranslation()
  const { mutateProfiles } = useProfiles()
  const [refreshing, setRefreshing] = useState(false)

  const refresh = useLockFn(async () => {
    if (!profile.uid) return
    setRefreshing(true)
    try {
      await updateProfile(profile.uid)
      await mutateProfiles()
      // clod: та же обратная связь, что у плитки «Обновить подписку»
      showNotice.success('home.components.subscription.updated')
    } catch (error) {
      showNotice.error(error)
    } finally {
      setRefreshing(false)
    }
  })

  // Read the clock once per mount: "expired" flipping a second later is not
  // worth an impure render (and the watcher notifies about expiry anyway).
  // clod: логотип берём из локального кэша, а не с чужого хоста: он не мигает
  // при старте, работает офлайн и не отдаёт IP пользователя при каждом показе.
  // Ключ включает время обновления подписки — сменился логотип, сменился кэш.
  const { data: cachedLogo, isLoading: logoLoading } = useSWR(
    profile.uid ? ['profileLogo', profile.uid, profile.updated ?? 0] : null,
    ([, uid]) => getProfileLogo(uid as string),
    { revalidateOnFocus: false },
  )
  // Пока кэш читается — не показываем ничего: подставить сюда URL из заголовка
  // значило бы сходить на хост провайдера ровно в тот момент, которого мы и
  // хотели избежать. URL остаётся фолбэком только когда кэша нет совсем.
  const logo = logoLoading ? undefined : (cachedLogo ?? profile.logo)

  const [now] = useState(() => Date.now())
  const expired =
    !!profile.extra?.expire && toUnixSeconds(profile.extra.expire) * 1000 < now

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
          {profile.name}
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
        disabled={refreshing}
        aria-label={t('shared.actions.refresh')}
        sx={{ borderRadius: '10px' }}
      >
        {refreshing ? <CircularProgress size={20} /> : <RefreshRoundedIcon />}
      </IconButton>
    </Stack>
  )
}
