import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  clockSkew,
  createUpdatesStore,
  missedUpdates,
  newerUpdatesInFlight,
  noServersReason,
  noServersSeverity,
} from './subscription-status.ts'

const DAY = 24 * 60 * 60
const now = () => Math.floor(Date.now() / 1000)

const profile = (fields: Partial<IProfileItem>): IProfileItem =>
  ({ uid: 'test', type: 'remote', ...fields }) as IProfileItem

describe('clockSkew', () => {
  it('берёт свежий замер и отбрасывает старый', () => {
    assert.equal(
      clockSkew(profile({ clock_skew: 42, clock_skew_at: now() - 60 })),
      42,
    )
    // Замер старше месяца не применяем: за это время пользователь мог
    // перевести часы, и прежняя поправка сама стала бы ошибкой того же размера.
    assert.equal(
      clockSkew(profile({ clock_skew: 42, clock_skew_at: now() - 31 * DAY })),
      undefined,
    )
  })

  it('отбрасывает замер из будущего', () => {
    // Отрицательный возраст — это часы, переведённые назад уже после замера,
    // то есть ровно тот случай, ради которого правило и заведено.
    assert.equal(
      clockSkew(profile({ clock_skew: 42, clock_skew_at: now() + 10 * DAY })),
      undefined,
    )
  })

  it('без замера считает по часам устройства', () => {
    assert.equal(clockSkew(profile({ clock_skew: 42 })), undefined)
    assert.equal(clockSkew(profile({ clock_skew_at: now() })), undefined)
    assert.equal(clockSkew(undefined), undefined)
  })
})

describe('noServersReason', () => {
  const extra = (over: Partial<IProfileItem['extra'] & object>) =>
    ({
      upload: 0,
      download: 0,
      total: 0,
      expire: 0,
      ...over,
    }) as NonNullable<IProfileItem['extra']>

  it('лимит устройств важнее всего остального', () => {
    // clod:stub-parity — на лимите устройств панель отвечает заглушками, но
    // `subscription-userinfo` в этом ответе ЗДОРОВЫЙ. Без приоритета экран
    // обвинил бы провайдера в том, что он «не выдал серверы».
    assert.equal(
      noServersReason(profile({ hwid_state: 'limit' })),
      'deviceLimit',
    )
    assert.equal(
      noServersReason(profile({ hwid_state: 'not_supported' })),
      'deviceLimit',
    )
    // Даже когда подписка вдобавок истекла — виновата не она.
    assert.equal(
      noServersReason(
        profile({
          hwid_state: 'limit',
          extra: extra({ expire: now() - DAY }),
        }),
      ),
      'deviceLimit',
    )
  })

  it('истёкший срок', () => {
    assert.equal(
      noServersReason(profile({ extra: extra({ expire: now() - 60 }) })),
      'expired',
    )
    // Ноль — это «бессрочно», а не «истекла в 1970».
    assert.notEqual(
      noServersReason(profile({ extra: extra({ expire: 0 }) })),
      'expired',
    )
  })

  it('исчерпанный трафик', () => {
    assert.equal(
      noServersReason(
        profile({ extra: extra({ upload: 6, download: 4, total: 10 }) }),
      ),
      'traffic',
    )
    // Безлимит (`total: 0`) исчерпать нельзя.
    assert.equal(
      noServersReason(
        profile({ extra: extra({ upload: 999, download: 999, total: 0 }) }),
      ),
      'provider',
    )
  })

  it('здоровая подписка без серверов — это к провайдеру', () => {
    assert.equal(
      noServersReason(
        profile({
          hwid_state: 'ok',
          extra: extra({ expire: now() + 30 * DAY, total: 100, download: 1 }),
        }),
      ),
      'provider',
    )
    assert.equal(noServersReason(undefined), 'provider')
  })
})

describe('missedUpdates', () => {
  const MIN = 60
  const HOUR = 60 * MIN
  const fetched = 1_000_000_000
  const remote = (fields: Partial<IProfileItem> = {}) =>
    profile({
      url: 'https://example.com/sub',
      updated: fetched,
      update_failed: true,
      option: { update_interval: 12 * 60 },
      ...fields,
    })

  it('ничего не пропущено, пока не прошли интервал и первый повтор', () => {
    const at = (seconds: number) => missedUpdates(remote(), fetched + seconds)
    assert.equal(at(12 * HOUR), 0)
    assert.equal(at(12 * HOUR + 15 * MIN - 1), 0)
    assert.equal(at(12 * HOUR + 15 * MIN), 1)
    assert.equal(at(24 * HOUR + 15 * MIN - 1), 1)
    assert.equal(at(24 * HOUR + 15 * MIN), 2)
    assert.equal(at(30 * 12 * HOUR), 2)
  })

  it('без провала последней попытки — не пропущено, сколько бы ни прошло', () => {
    const later = fetched + 30 * 24 * HOUR
    assert.equal(missedUpdates(remote({ update_failed: false }), later), 0)
    assert.equal(missedUpdates(remote({ update_failed: undefined }), later), 0)
  })

  it('без автообновления, без загрузки и у файла — не пропущено', () => {
    const later = fetched + 30 * 24 * HOUR
    assert.equal(missedUpdates(remote({ option: {} }), later), 0)
    assert.equal(
      missedUpdates(
        remote({ option: { update_interval: 60, allow_auto_update: false } }),
        later,
      ),
      0,
    )
    assert.equal(missedUpdates(remote({ updated: 0 }), later), 0)
    assert.equal(missedUpdates(remote({ url: undefined }), later), 0)
    assert.equal(missedUpdates(remote(), fetched - HOUR), 0)
    assert.equal(missedUpdates(undefined, later), 0)
  })
})

describe('noServersSeverity', () => {
  it('истёкшая подписка — ошибка, трафик и устройства — предупреждение', () => {
    assert.equal(noServersSeverity('expired'), 'error')
    assert.equal(noServersSeverity('traffic'), 'warning')
    assert.equal(noServersSeverity('deviceLimit'), 'warning')
    assert.equal(noServersSeverity('provider'), 'info')
  })
})

const snapshot = (revision: number, ...uids: string[]) => ({ revision, uids })

describe('newerUpdatesInFlight', () => {
  it('новый снимок применяется, в каком бы порядке ни пришли событие и ответ', () => {
    const started = snapshot(1, 'X')
    const finished = snapshot(2)
    const initial = snapshot(-1)
    // Событие о конце раньше ответа сверки, снятого до него, и наоборот.
    assert.equal(
      newerUpdatesInFlight(newerUpdatesInFlight(initial, finished), started),
      finished,
    )
    assert.equal(
      newerUpdatesInFlight(newerUpdatesInFlight(initial, started), finished),
      finished,
    )
  })

  it('повтор и старый снимок применённое не меняют', () => {
    const applied = snapshot(5, 'X')
    assert.equal(newerUpdatesInFlight(applied, snapshot(5)), applied)
    assert.equal(newerUpdatesInFlight(applied, snapshot(3, 'Y')), applied)
  })

  it('первый снимок нового окна применяется и с нулевым номером', () => {
    const first = snapshot(0)
    assert.equal(newerUpdatesInFlight(snapshot(-1), first), first)
  })
})

describe('createUpdatesStore', () => {
  it('идёт — пока подписка в снимке бэкенда; конец называет законченные', () => {
    const store = createUpdatesStore()
    assert.deepEqual(store.apply(snapshot(1, 'X', 'Y')), [])
    assert.equal(store.isUpdating('X'), true)
    assert.deepEqual(store.apply(snapshot(2, 'Y')), ['X'])
    assert.equal(store.isUpdating('X'), false)
    assert.equal(store.isUpdating('Y'), true)
  })

  it('опоздавший ответ сверки не зажигает законченное', () => {
    const store = createUpdatesStore()
    store.apply(snapshot(1, 'X'))
    store.apply(snapshot(2))
    assert.deepEqual(store.apply(snapshot(1, 'X')), [])
    assert.equal(store.isUpdating('X'), false)
  })

  it('свой вызов занимает подписку, пока не вернулся, и не путается со снимком', () => {
    const store = createUpdatesStore()
    store.beginOwn('X')
    assert.equal(store.isUpdating('X'), true, 'нажали — занято сразу')
    store.apply(snapshot(1, 'X'))
    store.apply(snapshot(2))
    assert.equal(store.isUpdating('X'), true, 'снимок не гасит свой вызов')
    store.endOwn('X')
    assert.equal(store.isUpdating('X'), false)

    store.apply(snapshot(3, 'X'))
    store.endOwn('X')
    assert.equal(
      store.isUpdating('X'),
      true,
      'конец своего вызова не гасит снимок',
    )
  })

  it('два своих вызова одной подписки: занято до конца последнего', () => {
    const store = createUpdatesStore()
    store.beginOwn('X')
    store.beginOwn('X')
    store.endOwn('X')
    assert.equal(store.isUpdating('X'), true)
    store.endOwn('X')
    assert.equal(store.isUpdating('X'), false)
  })

  it('подписчики слышат только перемены', () => {
    const store = createUpdatesStore()
    let heard = 0
    const unsubscribe = store.subscribe(() => {
      heard += 1
    })
    store.apply(snapshot(1, 'X'))
    store.apply(snapshot(1, 'X'))
    store.apply(snapshot(0))
    store.endOwn('Y')
    assert.equal(heard, 1)
    unsubscribe()
    store.apply(snapshot(2))
    assert.equal(heard, 1)
  })
})
