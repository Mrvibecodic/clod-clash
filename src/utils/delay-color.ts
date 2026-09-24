/**
 * One latency scale for the whole interface.
 *
 * A node that was never tested reads as unknown rather than bad, and a timeout
 * reads as an error — the two look the same as a number (`0` / `-1`) but mean
 * very different things to someone choosing a server.
 */
const GOOD_DELAY = 200
const FAIR_DELAY = 400

export const delayTone = (delay: number | undefined) => {
  if (delay === undefined || delay < 0) return undefined
  if (delay === 0 || delay >= FAIR_DELAY) return 'error'
  return delay < GOOD_DELAY ? 'success' : 'warning'
}

export const delayColor = (delay: number | undefined) => {
  const tone = delayTone(delay)
  return tone ? `${tone}.main` : 'text.disabled'
}

export const delayBars = (delay: number) =>
  delay < GOOD_DELAY / 2
    ? 4
    : delay < GOOD_DELAY
      ? 3
      : delay < FAIR_DELAY
        ? 2
        : 1
