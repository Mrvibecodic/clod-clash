const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB', 'ZB', 'YB']

/** Объём текстом, единица через пробел: «1.00 GB». */
const parseTraffic = (num?: number) => {
  if (typeof num !== 'number' || !Number.isFinite(num)) return 'NaN'
  const exp =
    num < 1 ? 0 : Math.min(Math.floor(Math.log2(num) / 10), UNITS.length - 1)
  const dat = num / Math.pow(1024, exp)
  const ret = Math.round(dat) >= 1000 ? dat.toFixed(0) : dat.toPrecision(3)
  const unit = UNITS[exp]

  return `${ret} ${unit}`
}

/** Скорость текстом: «1.00 MB/s». */
export const parseSpeed = (num?: number) => `${parseTraffic(num)}/s`

export default parseTraffic
