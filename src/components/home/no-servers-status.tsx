import CalendarMonthRoundedIcon from '@mui/icons-material/CalendarMonthRounded'
import DataUsageRoundedIcon from '@mui/icons-material/DataUsageRounded'
import InfoOutlinedIcon from '@mui/icons-material/InfoOutlined'
import { Alert, AlertTitle, Button, Stack, Typography } from '@mui/material'
import dayjs from 'dayjs'
import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

import { useNoServersStatus } from '@/hooks/use-no-servers-status'
import { openWebUrl, updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import parseTraffic from '@/utils/parse-traffic'
import { toUnixSeconds } from '@/utils/subscription-status'

interface Props {
  profile?: IProfileItem
  /** Refresh the profile list after a manual subscription update. */
  onRefreshed?: () => Promise<unknown> | void
}

const traffic = (bytes: number) => parseTraffic(bytes).join(' ').trim()

const date = (unix?: number) =>
  unix ? dayjs(toUnixSeconds(unix) * 1000).format('DD.MM.YYYY') : undefined

/**
 * clod: why there is nothing to connect to.
 *
 * An empty list is the honest result of the sentinel filter, but on its own it
 * explains nothing. The panel keeps sending real `subscription-userinfo` even
 * when it hands out placeholders instead of servers, so the reason is derived
 * from the subscription itself — and the actions are the ones the provider
 * already gave us (`clod-renew-url`, `clod-topup-url`, `support-url`). As
 * everywhere else in the app, a button exists only when its header does: the
 * app never invents a payment link.
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
  const expireDate = date(extra?.expire)
  const refillDate = date(profile.refill_date)

  const severity =
    reason === 'expired' ? 'error' : reason === 'traffic' ? 'warning' : 'info'

  const icon =
    reason === 'expired' ? (
      <CalendarMonthRoundedIcon fontSize="inherit" />
    ) : reason === 'traffic' ? (
      <DataUsageRoundedIcon fontSize="inherit" />
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
        {reason === 'traffic' && profile.topup_url ? (
          <Button
            size="small"
            variant="contained"
            color={severity}
            onClick={() => void openLink(profile.topup_url)}
          >
            {t('home.components.subscription.topup')}
          </Button>
        ) : null}
        {reason !== 'provider' && profile.renew_url ? (
          <Button
            size="small"
            variant={reason === 'expired' ? 'contained' : 'outlined'}
            color={severity}
            onClick={() => void openLink(profile.renew_url)}
          >
            {t('home.components.subscription.renew')}
          </Button>
        ) : null}
        {profile.support_url ? (
          <Button
            size="small"
            variant={reason === 'provider' ? 'contained' : 'text'}
            color={severity}
            onClick={() => void openLink(profile.support_url)}
          >
            {t('profiles.components.hwidDialog.support')}
          </Button>
        ) : null}
        {reason === 'provider' ? (
          <Button
            size="small"
            variant="outlined"
            color={severity}
            onClick={() => void refresh()}
          >
            {t('home.components.serverStatus.refresh')}
          </Button>
        ) : null}
      </Stack>
    </Alert>
  )
}
