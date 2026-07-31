import CalendarMonthRoundedIcon from '@mui/icons-material/CalendarMonthRounded'
import DataUsageRoundedIcon from '@mui/icons-material/DataUsageRounded'
import {
  Box,
  Button,
  LinearProgress,
  Stack,
  Typography,
} from '@mui/material'
import dayjs from 'dayjs'
import { useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { InfoTile } from '@/components/home/info-tile'
import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import parseTraffic from '@/utils/parse-traffic'

/** Colour of the traffic bar: green, then amber, then red as the quota runs out. */
const trafficColor = (usedPercent: number) => {
  if (usedPercent >= 90) return 'error' as const
  if (usedPercent >= 70) return 'warning' as const
  return 'success' as const
}

const DAY = 24 * 60 * 60

/**
 * Normalize a panel timestamp to unix seconds. Anything above ~1e12 can only
 * be milliseconds (that is the year 33658 in seconds) — some subscription
 * backends emit ms where the spec says seconds.
 */
const toUnixSeconds = (ts: number) => (ts > 1e12 ? Math.round(ts / 1000) : ts)

/** `parseTraffic` returns `[value, unit]`; join them the human way. */
const traffic = (bytes: number) => parseTraffic(bytes).join(' ').trim()

/** Days-left / traffic levels at which the plan counts as running out. */
export const CRITICAL_DAYS = 3
export const CRITICAL_TRAFFIC_PERCENT = 90

interface Props {
  profile: IProfileItem
}

/**
 * The subscription block: two equal tiles — traffic and time left — that sit
 * side by side and stack under each other when the window is narrow. The
 * renew / top-up actions live in the expiry tile and exist only if the panel
 * sent `clod-renew-url` / `clod-topup-url`; the app never invents payment
 * links on its own.
 */
export const SubscriptionCard = ({ profile }: Props) => {
  const { t } = useTranslation()

  const openLink = useCallback(async (url?: string) => {
    if (!url) return
    try {
      await openWebUrl(url)
    } catch (error) {
      showNotice.error(error)
    }
  }, [])

  const info = useMemo(() => {
    const extra = profile.extra
    if (!extra) return undefined

    const used = extra.upload + extra.download
    const unlimited = extra.total === 0
    // Some panels emit milliseconds where unix seconds are expected; a
    // timestamp past ~33658 AD in seconds can only be milliseconds.
    const expire = toUnixSeconds(extra.expire)
    const forever = !expire
    const usedPercent = unlimited
      ? 0
      : Math.min(100, Math.round((used * 100) / extra.total))
    const daysLeft = forever
      ? undefined
      : Math.max(0, Math.ceil((expire - Date.now() / 1000) / DAY))

    const critical =
      (daysLeft !== undefined && daysLeft <= CRITICAL_DAYS) ||
      (!unlimited && usedPercent >= CRITICAL_TRAFFIC_PERCENT)

    return {
      used,
      unlimited,
      forever,
      usedPercent,
      daysLeft,
      critical,
      expireDate: forever
        ? undefined
        : dayjs(expire * 1000).format('DD.MM.YYYY'),
      total: extra.total,
    }
  }, [profile.extra])

  // Nothing to report: an unlimited, never expiring plan hides the block
  // instead of showing a full bar that means nothing.
  if (!info || (info.unlimited && info.forever)) return null

  const showRenew = Boolean(profile.renew_url)
  const showTopup = Boolean(profile.topup_url)
  const expiryCritical = info.daysLeft !== undefined && info.daysLeft <= CRITICAL_DAYS

  return (
    <Box
      sx={{
        display: 'grid',
        gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))',
        gap: 1.25,
      }}
    >
      <InfoTile
        title={t('home.components.subscription.trafficTitle')}
        icon={<DataUsageRoundedIcon />}
      >
        {/* mockup: «12,4 ГБ / 100 ГБ» — used in bold, the quota greyed */}
        <Typography noWrap sx={{ fontSize: 15, fontWeight: 700 }}>
          {traffic(info.used)}{' '}
          <Typography
            component="span"
            sx={{ fontSize: 13.5, fontWeight: 500 }}
            color="text.secondary"
          >
            /{' '}
            {info.unlimited
              ? t('profiles.components.profileItem.labels.unlimited')
              : traffic(info.total)}
          </Typography>
        </Typography>
        {info.unlimited ? null : (
          <LinearProgress
            variant="determinate"
            value={info.usedPercent}
            color={trafficColor(info.usedPercent)}
            sx={{ height: 6, borderRadius: 3 }}
          />
        )}
        {/* The reset date is noise while the subscription itself is ending. */}
        {profile.refill_date && !info.critical ? (
          <Typography variant="caption" color="text.secondary" noWrap>
            {t('home.components.subscription.refill', {
              date: dayjs(toUnixSeconds(profile.refill_date) * 1000).format(
                'DD.MM.YYYY',
              ),
            })}
          </Typography>
        ) : null}
      </InfoTile>

      <InfoTile
        title={t('home.components.subscription.expiryTitle')}
        icon={<CalendarMonthRoundedIcon />}
      >
        {info.daysLeft !== undefined ? (
          <>
            <Typography
              noWrap
              sx={{ fontSize: 15, fontWeight: 700 }}
              color={expiryCritical ? 'error' : 'text.primary'}
            >
              {t('home.components.subscription.daysShort', {
                count: info.daysLeft,
              })}
            </Typography>
            <Typography
              variant="caption"
              noWrap
              color={expiryCritical ? 'error' : 'text.secondary'}
            >
              {t('home.components.subscription.untilDate', {
                date: info.expireDate,
              })}
            </Typography>
          </>
        ) : (
          <Typography noWrap sx={{ fontSize: 15, fontWeight: 700 }}>
            {t('profiles.components.profileItem.labels.neverExpires')}
          </Typography>
        )}
        {showRenew || showTopup ? (
          <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap', mt: 0.25 }}>
            {showRenew ? (
              <Button
                size="small"
                variant={info.critical ? 'contained' : 'outlined'}
                sx={{ minWidth: 0 }}
                onClick={() => void openLink(profile.renew_url)}
              >
                {t('home.components.subscription.renew')}
              </Button>
            ) : null}
            {showTopup ? (
              <Button
                size="small"
                variant="text"
                sx={{ minWidth: 0 }}
                onClick={() => void openLink(profile.topup_url)}
              >
                {t('home.components.subscription.topup')}
              </Button>
            ) : null}
          </Stack>
        ) : null}
      </InfoTile>
    </Box>
  )
}
