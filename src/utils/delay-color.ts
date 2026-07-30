/**
 * One latency scale for the whole simple interface.
 *
 * A node that was never tested reads as unknown rather than bad, and a timeout
 * reads as an error — the two look the same as a number (`0` / `-1`) but mean
 * very different things to someone choosing a server.
 */
export const delayColor = (delay: number | undefined) => {
  if (delay === undefined || delay < 0) return 'text.disabled'
  if (delay === 0) return 'error.main'
  if (delay < 150) return 'success.main'
  if (delay < 300) return 'warning.main'
  return 'error.main'
}
