import dayjs from 'dayjs'

/** Дата на экране профилей и у провайдеров: `YYYY-MM-DD`, без даты — «-». */
export const profileDate = (unix?: number) =>
  unix ? dayjs(unix * 1000).format('YYYY-MM-DD') : '-'

/**
 * Дата пополнения трафика на Главной: `DD.MM.YYYY`. Уже в секундах — её
 * приводит разбор ответа панели; поправку часов не применяем — так её
 * показывают все экраны Главной.
 */
export const refillDateText = (profile?: IProfileItem) =>
  profile?.refill_date
    ? dayjs(profile.refill_date * 1000).format('DD.MM.YYYY')
    : undefined

/**
 * Длительность сессии на кнопке подключения — как на Android: `MM:SS`, с часами
 * `H:MM:SS`.
 */
export const sessionTimeText = (seconds: number) => {
  const total = Math.max(0, Math.floor(seconds))
  const pad = (value: number) => String(value).padStart(2, '0')
  const hours = Math.floor(total / 3600)
  const rest = `${pad(Math.floor((total % 3600) / 60))}:${pad(total % 60)}`
  return hours > 0 ? `${hours}:${rest}` : rest
}
