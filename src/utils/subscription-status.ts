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
export type NoServersReason = 'expired' | 'traffic' | 'deviceLimit' | 'provider'

/** Насколько это серьёзно — один цвет на плашке и в строке шторки. */
export const noServersSeverity = (reason: NoServersReason) =>
  reason === 'expired'
    ? 'error'
    : reason === 'traffic' || reason === 'deviceLimit'
      ? 'warning'
      : 'info'

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

/** Запас на первый повтор после сбоя — как `FAILURE_RETRY` планировщика. */
const FIRST_RETRY_SECONDS = 15 * 60

/**
 * clod: сколько автообновлений подписки подряд пропущено к `nowSeconds` (часы
 * устройства): 0, 1 или 2 — больше не различаем. Счёт от последней удачной
 * загрузки `updated`, от неё же планировщик ведёт расписание; удачная загрузка,
 * своя или ручная, сдвигает её — и счёт обнуляется сам, хранить нечего.
 * Пропуском считается только сбой: пока последняя попытка не провалилась
 * (`update_failed`), ничего не пропущено — выключенный на выходные компьютер
 * подписку не «пропускал», и при включении пометка не мигает, пока идёт
 * обновление. Файловые подписки, подписки без автообновления и ни разу не
 * загруженные — 0.
 */
export const missedUpdates = (
  profile: IProfileItem | undefined,
  nowSeconds = Date.now() / 1000,
) => {
  const interval = (profile?.option?.update_interval ?? 0) * 60
  const updated = profile?.updated ?? 0
  if (
    !profile?.url ||
    !profile.update_failed ||
    profile.option?.allow_auto_update === false ||
    interval <= 0 ||
    updated <= 0
  )
    return 0

  const missed = Math.floor(
    (nowSeconds - updated - FIRST_RETRY_SECONDS) / interval,
  )
  return Math.min(2, Math.max(0, missed))
}

/** Сейчас по часам панели, в unix-секундах. */
const panelNow = (profile?: IProfileItem) =>
  Date.now() / 1000 + (clockSkew(profile) ?? 0)

export const noServersReason = (profile?: IProfileItem): NoServersReason => {
  // clod:stub-parity — лимит устройств проверяем ПЕРВЫМ: панель отвечает на
  // него заглушками, но `subscription-userinfo` в этом ответе здоровый —
  // подписка действует, трафик на месте. Без этой ветки экран обвинил бы
  // провайдера в том, что он «не выдал серверы».
  if (
    profile?.hwid_state === 'limit' ||
    profile?.hwid_state === 'not_supported'
  )
    return 'deviceLimit'

  const extra = profile?.extra
  if (extra) {
    const expire = extra.expire ?? 0
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

/**
 * Снимок бэкенда: какие подписки обновляются сейчас (кнопкой или
 * расписанием) и номер этого состояния. Номер растёт при каждой смене набора.
 */
export interface UpdatesInFlight {
  revision: number
  uids: string[]
}

/**
 * Из применённого и пришедшего снимков — более новый. Событие о смене и ответ
 * сверки идут разными путями и приходят в любом порядке; по номеру старое
 * не перебьёт новое.
 */
export const newerUpdatesInFlight = (
  applied: UpdatesInFlight,
  incoming: UpdatesInFlight,
) => (incoming.revision > applied.revision ? incoming : applied)

/**
 * «Идёт обновление» по подписке для всего окна: снимок бэкенда или свой вызов
 * окна, который ещё не вернулся (или ждёт очереди «Обновить все»). Два
 * признака не смешиваются: снимок меняет только бэкенд, свой вызов — только
 * тот, кто его начал.
 */
export const createUpdatesStore = () => {
  let applied: UpdatesInFlight = { revision: -1, uids: [] }
  const own = new Map<string, number>()
  const listeners = new Set<() => void>()
  const changed = () => {
    for (const listener of listeners) listener()
  }

  return {
    subscribe: (listener: () => void) => {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    isUpdating: (uid: string) => own.has(uid) || applied.uids.includes(uid),
    /** Применить снимок; ответ — подписки, обновление которых закончилось. */
    apply: (incoming: UpdatesInFlight): string[] => {
      const next = newerUpdatesInFlight(applied, incoming)
      if (next === applied) return []
      const finished = applied.uids.filter((uid) => !next.uids.includes(uid))
      applied = next
      changed()
      return finished
    },
    beginOwn: (uid: string) => {
      own.set(uid, (own.get(uid) ?? 0) + 1)
      changed()
    },
    endOwn: (uid: string) => {
      const calls = own.get(uid)
      if (calls === undefined) return
      if (calls > 1) {
        own.set(uid, calls - 1)
      } else {
        own.delete(uid)
      }
      changed()
    },
  }
}
