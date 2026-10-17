const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB']

let decimalSeparator = '.'

export const setTrafficLocale = (locale: string) => {
  try {
    decimalSeparator =
      (1.5)
        .toLocaleString(locale)
        .replace(/\p{Nd}/gu, '')
        .charAt(0) || '.'
  } catch {
    decimalSeparator = '.'
  }
}

/**
 * Объём текстом, единица через пробел: «1.00 GB» (с запятой — по языку). От
 * тысячи — уже следующая единица: «0.98 MB», а не «1018 KB».
 */
const parseTraffic = (num?: number) => {
  if (typeof num !== 'number' || !Number.isFinite(num)) return 'NaN'
  let exp =
    num < 1 ? 0 : Math.min(Math.floor(Math.log2(num) / 10), UNITS.length - 1)
  let dat = num / Math.pow(1024, exp)
  if (dat >= 999.5 && exp < UNITS.length - 1) {
    exp += 1
    dat /= 1024
  }
  const ret = Math.round(dat) >= 1000 ? dat.toFixed(0) : dat.toPrecision(3)
  const unit = UNITS[exp]

  return `${ret.replace('.', decimalSeparator)} ${unit}`
}

/** Скорость текстом: «1.00 MB/s». */
export const parseSpeed = (num?: number) => `${parseTraffic(num)}/s`

export default parseTraffic
