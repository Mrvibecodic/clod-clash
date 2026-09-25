/**
 * One latency scale for the whole interface.
 *
 * A node that was never tested reads as unknown rather than bad, and a timeout
 * reads as an error — the two look the same as a number (`0` / `-1`) but mean
 * very different things to someone choosing a server.
 */
const GOOD_DELAY = 200
const FAIR_DELAY = 400

type DelayBounds = readonly [good: number, fair: number]

const DEFAULT_BOUNDS: DelayBounds = [GOOD_DELAY, FAIR_DELAY]

export const delayTone = (
  delay: number | undefined,
  [good, fair]: DelayBounds = DEFAULT_BOUNDS,
) => {
  if (delay === undefined || delay < 0) return undefined
  if (delay === 0 || delay >= fair) return 'error'
  return delay < good ? 'success' : 'warning'
}

export const delayColor = (delay: number | undefined, bounds?: DelayBounds) => {
  const tone = delayTone(delay, bounds)
  return tone ? `${tone}.main` : 'text.disabled'
}

export const delayBars = (
  delay: number,
  [good, fair]: DelayBounds = DEFAULT_BOUNDS,
) => (delay < good / 2 ? 4 : delay < good ? 3 : delay < fair ? 2 : 1)
