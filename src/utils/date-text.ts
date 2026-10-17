import dayjs, { type Dayjs } from 'dayjs'

const DATE = 'DD.MM.YYYY'

/** Дата в интерфейсе — одна на все экраны: `DD.MM.YYYY`, без даты — «-». */
export const profileDate = (unix?: number) =>
  unix ? dayjs(unix * 1000).format(DATE) : '-'

/** Та же дата со временем — где за день бывает несколько записей. */
export const dateTimeText = (time: Dayjs) => time.format(`${DATE} HH:mm`)

/**
 * Дата пополнения трафика на Главной. Уже в секундах — её приводит разбор
 * ответа панели; поправку часов не применяем — так её показывают все экраны.
 */
export const refillDateText = (profile?: IProfileItem) =>
  profile?.refill_date ? profileDate(profile.refill_date) : undefined

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
