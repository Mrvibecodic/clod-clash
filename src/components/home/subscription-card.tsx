import { Button, Chip, LinearProgress, Stack, Typography } from '@mui/material'
import dayjs from 'dayjs'
import { useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

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

/** Days-left / traffic levels at which the plan counts as running out. */
export const CRITICAL_DAYS = 3
export const CRITICAL_TRAFFIC_PERCENT = 90

interface Props {
  profile: IProfileItem
}

/**
 * The subscription card: traffic, time left and — when the panel provides the
 * URLs — the renew / top-up actions.
 *
 * Expiry and the monthly traffic reset are deliberately kept apart: "expires
 * in 2 days" next to "traffic resets on the 15th" reads as a contradiction,
 * so the reset line is dropped whenever the plan itself is about to end.
 *
 * The action buttons exist only if the panel sent `clod-renew-url` /
 * `clod-topup-url`. No headers — no buttons; the app never invents payment
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
    const forever = !extra.expire
    const usedPercent = unlimited
      ? 0
      : Math.min(100, Math.round((used * 100) / extra.total))
    const daysLeft = forever
      ? undefined
      : Math.max(0, Math.ceil((extra.expire - Date.now() / 1000) / DAY))

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
        : dayjs(extra.expire * 1000).format('DD.MM.YYYY'),
      total: extra.total,
    }
  }, [profile.extra])

  // Nothing to report: an unlimited, never expiring plan hides the card
  // instead of showing a full bar that means nothing.
  if (!info || (info.unlimited && info.forever)) return null

  const showRenew = Boolean(profile.renew_url)
  const showTopup = Boolean(profile.topup_url)

  return (
    <Stack
      sx={{
        gap: 1,
        p: 2,
        borderRadius: 2,
        border: (theme) => `1px solid ${theme.palette.divider}`,
      }}
    >
      <Stack
        direction="row"
        sx={{ alignItems: 'center', gap: 1, flexWrap: 'wrap' }}
      >
        <Typography variant="body2" sx={{ flex: 1 }}>
          {parseTraffic(info.used)} /{' '}
          {info.unlimited
            ? t('profiles.components.profileItem.labels.unlimited')
            : parseTraffic(info.total)}
        </Typography>
        {info.daysLeft !== undefined ? (
          <Chip
            size="small"
            color={info.daysLeft <= CRITICAL_DAYS ? 'error' : 'default'}
            label={t('home.components.subscription.expires', {
              count: info.daysLeft,
              date: info.expireDate,
            })}
          />
        ) : (
          <Chip
            size="small"
            label={t('profiles.components.profileItem.labels.neverExpires')}
          />
        )}
      </Stack>

      {info.unlimited ? null : (
        <LinearProgress
          variant="determinate"
          value={info.usedPercent}
          color={trafficColor(info.usedPercent)}
        />
      )}

      {/* The reset date is noise while the subscription itself is ending. */}
      {profile.refill_date && !info.critical ? (
        <Typography variant="caption" color="text.secondary">
          {t('home.components.subscription.refill', {
            date: dayjs(profile.refill_date * 1000).format('DD.MM.YYYY'),
          })}
        </Typography>
      ) : null}

      {showRenew || showTopup ? (
        <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap', mt: 0.5 }}>
          {showRenew ? (
            <Button
              size="small"
              variant={info.critical ? 'contained' : 'outlined'}
              onClick={() => void openLink(profile.renew_url)}
            >
              {t('home.components.subscription.renew')}
            </Button>
          ) : null}
          {showTopup ? (
            <Button
              size="small"
              variant="text"
              onClick={() => void openLink(profile.topup_url)}
            >
              {t('home.components.subscription.topup')}
            </Button>
          ) : null}
        </Stack>
      ) : null}
    </Stack>
  )
}
