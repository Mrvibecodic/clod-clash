/**
 * clod: why the server list is empty.
 *
 * Remnawave answers an expired subscription, an exhausted quota, a disabled
 * user and unconfigured hosts with HTTP 200 and a config made of placeholder
 * nodes — the sentinel filter drops those, and the list ends up empty. The
 * reason is derived from `subscription-userinfo`, which stays truthful in all
 * of these responses; the placeholders' names are the panel admin's own text
 * in a language of their choosing and are only ever quoted, never parsed.
 */
export type NoServersReason = 'expired' | 'traffic' | 'provider'

/**
 * Normalize a panel timestamp to unix seconds. Anything above ~1e12 can only
 * be milliseconds (that is the year 33658 in seconds) — some subscription
 * backends emit ms where the spec says seconds.
 */
export const toUnixSeconds = (ts: number) =>
  ts > 1e12 ? Math.round(ts / 1000) : ts

export const noServersReason = (profile?: IProfileItem): NoServersReason => {
  const extra = profile?.extra
  if (extra) {
    const expire = toUnixSeconds(extra.expire ?? 0)
    if (expire > 0 && expire * 1000 <= Date.now()) return 'expired'

    const total = extra.total ?? 0
    const used = (extra.upload ?? 0) + (extra.download ?? 0)
    if (total > 0 && used >= total) return 'traffic'
  }
  // Срок и трафик в порядке, а серверов нет: отключённая подписка или
  // ненастроенные хосты — по данным подписки их не различить.
  return 'provider'
}
