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
