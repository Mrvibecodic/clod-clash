/// <reference types="node" />
// clod:tests — типы Node подключаем ТОЛЬКО здесь, директивой. В `tsconfig`
// стоит явный список `types`, и добавление в него «node» дало бы node-глобалы
// всему фронту: `setTimeout` начал бы возвращать `NodeJS.Timeout` вместо
// `number` и переломал бы таймеры в компонентах.

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  clockSkew,
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
