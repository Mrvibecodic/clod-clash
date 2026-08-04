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

/**
 * clod: поправка к часам устройства до времени панели, в секундах.
 * `undefined` — часов панели мы не знаем и считаем по своим.
 *
 * Значение снято из заголовка `Date` при обновлении подписки
 * (`PrfItem::clock_skew`) и лежит в профиле, поэтому поправка работает и
 * офлайн. Но только пока она свежая: часы устройства пользователь может
 * поправить руками, да и синхронизация времени после загрузки делает то же
 * самое — и тогда старая поправка сама станет ошибкой ровно того же размера.
 * Кварц за месяц уходит на секунды, так что рискуем мы не дрейфом, а
 * переводом часов: измерение старше месяца не применяем и честно говорим, что
 * считаем по устройству.
 */
const SKEW_MAX_AGE_SECONDS = 30 * 24 * 60 * 60

export const clockSkew = (profile?: IProfileItem): number | undefined => {
  const skew = profile?.clock_skew
  const measuredAt = profile?.clock_skew_at
  if (skew === undefined || measuredAt === undefined) return undefined

  // Возраст считаем от момента ЗАМЕРА, а не от `updated`: обновление подписки
  // без заголовка `Date` двигает `updated`, но поправку не трогает, и старый
  // замер иначе выглядел бы вечно свежим. Отрицательный возраст — это часы,
  // переведённые назад под уже снятой поправкой, то есть ровно тот случай,
  // ради которого правило и заведено.
  const age = Date.now() / 1000 - measuredAt
  return age < 0 || age > SKEW_MAX_AGE_SECONDS ? undefined : skew
}

/** Сейчас по часам панели, в unix-секундах. */
export const panelNow = (profile?: IProfileItem) =>
  Date.now() / 1000 + (clockSkew(profile) ?? 0)

export const noServersReason = (profile?: IProfileItem): NoServersReason => {
  const extra = profile?.extra
  if (extra) {
    const expire = toUnixSeconds(extra.expire ?? 0)
    // Срок — абсолютный момент, поэтому сверяем его с часами панели: иначе
    // экран «нет серверов» и карточка подписки ответят на один и тот же
    // вопрос по-разному.
    if (expire > 0 && expire <= panelNow(profile)) return 'expired'

    const total = extra.total ?? 0
    const used = (extra.upload ?? 0) + (extra.download ?? 0)
    if (total > 0 && used >= total) return 'traffic'
  }
  // Срок и трафик в порядке, а серверов нет: отключённая подписка или
  // ненастроенные хосты — по данным подписки их не различить.
  return 'provider'
}
