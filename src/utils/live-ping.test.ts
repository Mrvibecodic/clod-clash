import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { BatchQueue } from './batch-queue.ts'
import {
  firstMeasureCounts,
  groupTestAhead,
  livePingDue,
  nodesLeftToTest,
  PING_GAP_MS,
} from './live-ping.ts'

const NOW = 10 * PING_GAP_MS
const stale = { now: NOW, measuredAt: 0, lastPingAt: 0, hasPing: true }

describe('живой пинг Главной', () => {
  it('не шлёт свой запрос, пока текущий узел меряет пакет', () => {
    const key = 'Прокси|1|a\nb'
    assert.equal(
      livePingDue({ ...stale, batchOwns: groupTestAhead(key, '') }),
      false,
    )
    assert.equal(livePingDue({ ...stale, batchOwns: true }), false)
    assert.equal(
      livePingDue({ ...stale, batchOwns: groupTestAhead(key, key) }),
      true,
    )
    assert.equal(
      livePingDue({ ...stale, batchOwns: groupTestAhead('', '') }),
      true,
    )
    assert.equal(
      livePingDue({ ...stale, measuredAt: NOW - 1000, batchOwns: false }),
      false,
    )
  })
})

describe('автозамер группы и текущий узел', () => {
  const nodes = [{ name: 'a' }, { name: 'b' }, { name: 'c' }]

  it('узел, чей первый замер идёт или снят, пакет второй раз не меряет', () => {
    assert.equal(firstMeasureCounts(-2), true)
    assert.equal(firstMeasureCounts(120), true)
    assert.equal(firstMeasureCounts(0), true)
    assert.deepEqual(
      nodesLeftToTest(nodes, 'a').map((node) => node.name),
      ['b', 'c'],
    )
  })

  it('без замера или после отказа вызова пакет перемеряет текущий узел', () => {
    assert.equal(firstMeasureCounts(undefined), false)
    assert.equal(firstMeasureCounts(-1), false)
    assert.equal(nodesLeftToTest(nodes, undefined).length, 3)
  })
})

describe('очередь пакетных проверок', () => {
  it('закончивший пакет не снимает узел, поставленный позже другим', () => {
    const queue = new BatchQueue()
    const older = queue.enqueue(['g::a', 'g::b'])
    const newer = queue.enqueue(['g::a'])
    queue.dequeue('g::a', older)
    assert.equal(queue.has('g::a'), true)
    queue.dequeue('g::b', older)
    assert.equal(queue.has('g::b'), false)
    queue.dequeue('g::a', newer)
    assert.equal(queue.has('g::a'), false)
  })
})
