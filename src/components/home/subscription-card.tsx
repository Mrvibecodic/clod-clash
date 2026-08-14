import CalendarMonthRoundedIcon from '@mui/icons-material/CalendarMonthRounded'
import DataUsageRoundedIcon from '@mui/icons-material/DataUsageRounded'
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded'
import WarningAmberRoundedIcon from '@mui/icons-material/WarningAmberRounded'
import {
  alpha,
  Box,
  CircularProgress,
  IconButton,
  LinearProgress,
  Stack,
  Tooltip,
  Typography,
} from '@mui/material'
import dayjs from 'dayjs'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { InfoTile } from '@/components/home/info-tile'
import { useExpiryCountdown } from '@/hooks/use-expiry-countdown'
import { useTrafficEstimate } from '@/hooks/use-traffic-estimate'
import { CARD_VALUE } from '@/pages/_theme'
import parseTraffic from '@/utils/parse-traffic'
import { clockSkew, toUnixSeconds } from '@/utils/subscription-status'

/** Colour of the traffic bar: green, then amber, then red as the quota runs out. */
const trafficColor = (usedPercent: number) => {
  if (usedPercent >= 90) return 'error' as const
  if (usedPercent >= 70) return 'warning' as const
  return 'success' as const
}

const DAY = 24 * 60 * 60

/** `parseTraffic` returns `[value, unit]`; join them the human way. */
const traffic = (bytes: number) => parseTraffic(bytes).join(' ').trim()

/** Days-left / traffic levels at which the plan counts as running out. */
const CRITICAL_DAYS = 3
const CRITICAL_TRAFFIC_PERCENT = 90

interface Props {
  profile: IProfileItem
}

/**
 * The subscription block: two equal tiles — traffic and time left — that sit
 * side by side and stack under each other when the window is narrow.
 *
 * clod: платёжных действий здесь нет. Куда идти платить, знает только
 * провайдер, и ведёт туда единственная ссылка `clod-portal-url` — кнопка
 * «Личный кабинет» рядом с поддержкой.
 */
export const SubscriptionCard = ({ profile }: Props) => {
  const { t } = useTranslation()
  // clod: панель пересчитывает расход не чаще раза в час — то, что клиент
  // досчитал после неё, идёт ТОЛЬКО в показываемое число и в хвост полосы.
  // Пороги `critical` и «трафик закончился» считаются строго по данным
  // подписки, иначе клиент соврёт при живых серверах.
  const { estimate, refreshing, refresh } = useTrafficEstimate(profile)
  // clod: срок считается на клиенте и потому работает офлайн; часы устройства
  // при этом сдвинуты на разницу с часами панели, снятую при последнем
  // обновлении подписки. Подробности — в `use-expiry-countdown.ts`.
  // Some panels emit milliseconds where unix seconds are expected; a timestamp
  // past ~33658 AD in seconds can only be milliseconds.
  const expire = toUnixSeconds(profile.extra?.expire ?? 0)
  const skew = clockSkew(profile)
  const countdown = useExpiryCountdown(expire, skew ?? 0)

  const info = useMemo(() => {
    const extra = profile.extra
    if (!extra) return undefined

    const used = extra.upload + extra.download
    const unlimited = extra.total === 0
    const forever = !expire
    const usedPercent = unlimited
      ? 0
      : Math.min(100, Math.round((used * 100) / extra.total))

    return {
      used,
      unlimited,
      forever,
      usedPercent,
      // В часовом режиме показываем время, а не дату: «до 03.02.2027» рядом с
      // «4 ч» ничего не добавляет, а «до 21:40» отвечает на сам вопрос.
      //
      // Момент истечения переводим в часы устройства (`- skew`), а не в
      // абсолютное время: иначе на одной плитке «4 ч» считалось бы по часам
      // панели, а «до 21:40» читалось бы по часам пользователя, и два числа
      // расходились бы ровно на поправку. Что часы разошлись, говорит значок
      // рядом с числом.
      expireDate: forever
        ? undefined
        : dayjs((expire - (skew ?? 0)) * 1000).format('DD.MM.YYYY'),
      expireTime: forever
        ? undefined
        : dayjs((expire - (skew ?? 0)) * 1000).format('HH:mm'),
      total: extra.total,
    }
  }, [profile.extra, expire, skew])

  // Nothing to report: an unlimited, never expiring plan hides the block
  // instead of showing a full bar that means nothing.
  if (!info || (info.unlimited && info.forever)) return null

  // clod: часы вместо дней — только последние сутки. Оговорка про часы висит
  // именно там: день округляется честно в любом случае.
  const hourly = !info.forever && countdown.hoursLeft !== undefined
  // clod: «до 20:33» не отвечает на вопрос «а это когда» — сегодня вечером или
  // завтра днём. В часовом режиме до срока меньше суток, значит это ровно один
  // из двух дней; называем его прямо. Считаем по тем же часам, что и время на
  // плитке (момент истечения, переведённый в часы устройства), иначе подпись
  // разошлась бы с числом рядом.
  const daysUntilExpiry = info.forever
    ? undefined
    : dayjs((expire - (skew ?? 0)) * 1000)
        .startOf('day')
        .diff(dayjs().startOf('day'), 'day')
  const untilTimeKey =
    daysUntilExpiry === 0
      ? 'home.components.subscription.untilTimeToday'
      : daysUntilExpiry === 1
        ? 'home.components.subscription.untilTimeTomorrow'
        : 'home.components.subscription.untilTime'
  const expired = !info.forever && countdown.secondsLeft <= 0
  // Часы устройства сверены с панелью — говорить, что «считаем по вашим
  // часам», больше нечего. Расхождение крупнее пяти минут пользователь увидит
  // сам: «до 21:40» под числом — это время панели, а не то, что на его часах.
  const clockOff = skew !== undefined && Math.abs(skew) > 5 * 60
  const clockCaveat = skew === undefined || clockOff
  const expiryCritical =
    !info.forever && countdown.secondsLeft <= CRITICAL_DAYS * DAY
  const critical =
    expiryCritical ||
    (!info.unlimited && info.usedPercent >= CRITICAL_TRAFFIC_PERCENT)

  // Сумма «подписка + досчитанное клиентом» и её доля — только для показа.
  // Процент режем сотней, чтобы штриховка не выехала за полосу.
  const approximate = estimate.approximate
  const shownUsed = info.used + (approximate ? estimate.localBytes : 0)
  const shownPercent = info.unlimited
    ? 0
    : Math.min(100, Math.round((shownUsed * 100) / info.total))

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
        <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5 }}>
          <Typography noWrap sx={CARD_VALUE}>
            {approximate ? '≈ ' : ''}
            {traffic(shownUsed)}{' '}
            <Typography
              component="span"
              sx={{
                fontSize: 13.5,
                fontWeight: 500,
                fontVariantNumeric: 'tabular-nums',
              }}
              color="text.secondary"
            >
              /{' '}
              {info.unlimited
                ? t('profiles.components.profileItem.labels.unlimited')
                : traffic(info.total)}
            </Typography>
          </Typography>
          {approximate ? (
            <>
              <Tooltip
                title={
                  <>
                    {t('home.components.subscription.approximate.hint')}
                    {estimate.baselineAt ? (
                      <Box component="span" sx={{ display: 'block', mt: 0.5 }}>
                        {t('home.components.subscription.approximate.since', {
                          time: dayjs(estimate.baselineAt * 1000).format(
                            'HH:mm',
                          ),
                        })}
                      </Box>
                    ) : null}
                  </>
                }
              >
                <WarningAmberRoundedIcon
                  sx={{ fontSize: 16, color: 'warning.main', flex: 'none' }}
                />
              </Tooltip>
              <IconButton
                size="small"
                sx={{ p: 0.25 }}
                disabled={refreshing}
                aria-label={t(
                  'home.components.subscription.approximate.refresh',
                )}
                title={t('home.components.subscription.approximate.refresh')}
                onClick={() => void refresh()}
              >
                {refreshing ? (
                  <CircularProgress size={14} />
                ) : (
                  <RefreshRoundedIcon sx={{ fontSize: 15 }} />
                )}
              </IconButton>
            </>
          ) : null}
        </Stack>
        {info.unlimited ? null : approximate ? (
          // Две части в одной полосе: сплошная — подтверждённое подпиской,
          // штриховка — досчитанное клиентом. Видно, какая часть на честном
          // слове. `LinearProgress` двух сегментов не умеет.
          <Box
            sx={{
              height: 6,
              borderRadius: 999,
              overflow: 'hidden',
              display: 'flex',
              bgcolor: 'action.hover',
            }}
          >
            <Box
              sx={{
                width: `${info.usedPercent}%`,
                bgcolor: `${trafficColor(shownPercent)}.main`,
              }}
            />
            <Box
              sx={(theme) => ({
                width: `${Math.max(0, shownPercent - info.usedPercent)}%`,
                backgroundImage: `repeating-linear-gradient(45deg, ${theme.palette.warning.main} 0 3px, ${alpha(theme.palette.warning.main, 0.25)} 3px 6px)`,
              })}
            />
          </Box>
        ) : (
          <LinearProgress
            variant="determinate"
            value={info.usedPercent}
            color={trafficColor(info.usedPercent)}
            sx={{ height: 6, borderRadius: 999 }}
          />
        )}
        {/* The reset date is noise while the subscription itself is ending. */}
        {profile.refill_date && !critical ? (
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
        {!info.forever ? (
          <>
            <Stack direction="row" sx={{ alignItems: 'center', gap: 0.5 }}>
              <Typography
                noWrap
                sx={CARD_VALUE}
                color={expiryCritical ? 'error' : 'text.primary'}
              >
                {expired
                  ? t('home.components.subscription.expiredShort')
                  : hourly
                    ? t('home.components.subscription.hoursShort', {
                        count: countdown.hoursLeft,
                      })
                    : t('home.components.subscription.daysShort', {
                        count: countdown.daysLeft,
                      })}
              </Typography>
              {/* Часы клиент считает сам — про это надо сказать теми же
                  словами и тем же значком, что и про трафик. Если часы
                  устройства сверены с панелью и сходятся, говорить нечего. */}
              {hourly && !expired && clockCaveat ? (
                <Tooltip
                  title={t(
                    clockOff
                      ? 'home.components.subscription.expiryClockOff'
                      : 'home.components.subscription.expiryApproximate',
                  )}
                >
                  <WarningAmberRoundedIcon
                    sx={{ fontSize: 16, color: 'warning.main', flex: 'none' }}
                  />
                </Tooltip>
              ) : null}
            </Stack>
            <Typography
              variant="caption"
              noWrap
              color={expiryCritical ? 'error' : 'text.secondary'}
            >
              {/* Уже истёкшая подписка называет дату: одно время без даты у
                  того, кто не заходил неделю, не значит ничего. */}
              {hourly && !expired
                ? t(untilTimeKey, {
                    time: info.expireTime,
                  })
                : t('home.components.subscription.untilDate', {
                    date: info.expireDate,
                  })}
            </Typography>
          </>
        ) : (
          <Typography noWrap sx={CARD_VALUE}>
            {t('profiles.components.profileItem.labels.neverExpires')}
          </Typography>
        )}
      </InfoTile>
    </Box>
  )
}
