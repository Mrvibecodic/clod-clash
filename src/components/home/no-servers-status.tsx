import CalendarMonthRoundedIcon from '@mui/icons-material/CalendarMonthRounded'
import DataUsageRoundedIcon from '@mui/icons-material/DataUsageRounded'
import DevicesRoundedIcon from '@mui/icons-material/DevicesRounded'
import InfoOutlinedIcon from '@mui/icons-material/InfoOutlined'
import { Alert, AlertTitle, Button, Stack, Typography } from '@mui/material'
import dayjs from 'dayjs'
import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

import { useNoServersStatus } from '@/hooks/use-no-servers-status'
import { openWebUrl, updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import parseTraffic from '@/utils/parse-traffic'
import { clockSkew, toUnixSeconds } from '@/utils/subscription-status'

interface Props {
  profile?: IProfileItem
  /** Refresh the profile list after a manual subscription update. */
  onRefreshed?: () => Promise<unknown> | void
}

const traffic = (bytes: number) => parseTraffic(bytes).join(' ').trim()

// clod: дату ИСТЕЧЕНИЯ показываем в часах устройства (`- skew`) — так же, как
// карточка подписки, иначе при разошедшихся часах два экрана назовут разные
// дни. Дату пополнения не сдвигаем: её рисуют ещё три экрана без поправки, и
// расхождение внутри одного окна было бы хуже, чем сдвиг в сутки у того, у
// кого часы и так врут.
const date = (unix?: number, skew = 0) =>
  unix
    ? dayjs((toUnixSeconds(unix) - skew) * 1000).format('DD.MM.YYYY')
    : undefined

/**
 * clod: why there is nothing to connect to.
 *
 * An empty list is the honest result of the sentinel filter, but on its own it
 * explains nothing. The panel keeps sending real `subscription-userinfo` even
 * when it hands out placeholders instead of servers, so the reason is derived
 * from the subscription itself. Действие остаётся одно — написать в поддержку
 * (`support-url`), и кнопка есть только тогда, когда провайдер прислал этот
 * заголовок: своих ссылок приложение не выдумывает.
 */
export const NoServersStatus = ({ profile, onRefreshed }: Props) => {
  const { t } = useTranslation()
  const { reason, show, remarks } = useNoServersStatus(profile)

  const openLink = useCallback(async (url?: string) => {
    if (!url) return
    try {
      await openWebUrl(url)
    } catch (error) {
      showNotice.error(error)
    }
  }, [])

  const refresh = useCallback(async () => {
    if (!profile?.uid) return
    try {
      await updateProfile(profile.uid)
      await onRefreshed?.()
      showNotice.success('home.components.subscription.updated')
    } catch (error) {
      showNotice.error(error)
    }
  }, [profile?.uid, onRefreshed])

  // Nothing to explain: the list is empty for a reason we do not know (a
  // template without groups, a core that has not started yet).
  if (!profile || !show) return null

  const extra = profile.extra
  const used = (extra?.upload ?? 0) + (extra?.download ?? 0)
  const skew = clockSkew(profile) ?? 0
  const expireDate = date(extra?.expire, skew)
  const refillDate = date(profile.refill_date)

  const severity =
    reason === 'expired'
      ? 'error'
      : reason === 'traffic' || reason === 'deviceLimit'
        ? 'warning'
        : 'info'

  const icon =
    reason === 'expired' ? (
      <CalendarMonthRoundedIcon fontSize="inherit" />
    ) : reason === 'traffic' ? (
      <DataUsageRoundedIcon fontSize="inherit" />
    ) : reason === 'deviceLimit' ? (
      <DevicesRoundedIcon fontSize="inherit" />
    ) : (
      <InfoOutlinedIcon fontSize="inherit" />
    )

  const title = t(`home.components.serverStatus.title.${reason}`)

  const body =
    reason === 'expired'
      ? expireDate
        ? t('home.components.serverStatus.body.expired', { date: expireDate })
        : t('home.components.serverStatus.body.expiredNoDate')
      : reason === 'traffic'
        ? refillDate
          ? t('home.components.serverStatus.body.traffic', {
              used: traffic(used),
              total: traffic(extra?.total ?? 0),
              date: refillDate,
            })
          : t('home.components.serverStatus.body.trafficNoDate', {
              used: traffic(used),
              total: traffic(extra?.total ?? 0),
            })
        : reason === 'deviceLimit'
          ? profile.hwid_state === 'not_supported'
            ? t('home.components.serverStatus.body.deviceNotIdentified')
            : t('home.components.serverStatus.body.deviceLimit', {
                // clod: НЕ `count` — i18next считает его плюральным и полез бы
                // за ключами `_one`/`_other`, которых наш генератор не делает.
                max: profile.hwid_max_devices ?? 0,
              })
          : t('home.components.serverStatus.body.provider')

  // The panel's own words for the nodes it sent instead of servers: the only
  // hint available when the subscription data itself looks healthy.
  const panelSays =
    reason === 'provider' ? remarks.filter(Boolean).join(' · ') : ''

  return (
    <Alert
      severity={severity}
      icon={icon}
      sx={{ '& .MuiAlert-message': { width: '100%' } }}
    >
      <AlertTitle sx={{ fontSize: 14, fontWeight: 600, mb: 0.25 }}>
        {title}
      </AlertTitle>
      <Typography variant="body2">{body}</Typography>
      {panelSays ? (
        <Typography
          variant="caption"
          sx={{ display: 'block', fontStyle: 'italic', opacity: 0.8, mt: 0.25 }}
        >
          {t('home.components.serverStatus.panelSays', { text: panelSays })}
        </Typography>
      ) : null}

      <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap', mt: 1 }}>
        {/* clod: платёжных кнопок здесь нет — в приложении их нет нигде.
            Остаются поддержка и перечитать подписку. */}
        {profile.support_url ? (
          <Button
            size="small"
            variant="contained"
            color={severity}
            onClick={() => void openLink(profile.support_url)}
          >
            {t('profiles.components.hwidDialog.support')}
          </Button>
        ) : null}
        {/* clod: перечитать подписку полезно во всех трёх состояниях —
            продлил в кабинете, вернулся, нажал. Раньше кнопка была только у
            «провайдер не выдал серверы», потому что рядом стояли платёжные. */}
        <Button
          size="small"
          variant="outlined"
          color={severity}
          onClick={() => void refresh()}
        >
          {t('home.components.serverStatus.refresh')}
        </Button>
      </Stack>
    </Alert>
  )
}
