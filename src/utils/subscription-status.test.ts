import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  clockSkew,
  missedUpdates,
  noServersReason,
  toUnixSeconds,
} from './subscription-status.ts'

const DAY = 24 * 60 * 60
const now = () => Math.floor(Date.now() / 1000)

const profile = (fields: Partial<IProfileItem>): IProfileItem =>
  ({ uid: 'test', type: 'remote', ...fields }) as IProfileItem

describe('toUnixSeconds', () => {
  it('распознаёт миллисекунды по величине', () => {
    // Панели встречаются и те, что шлют миллисекунды там, где спека говорит
    // секунды. Порог 1e12 — это год 33658 в секундах, спутать не с чем.
    assert.equal(toUnixSeconds(1_754_000_000), 1_754_000_000)
    assert.equal(toUnixSeconds(1_754_000_000_000), 1_754_000_000)
    assert.equal(toUnixSeconds(0), 0)
  })
})

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
