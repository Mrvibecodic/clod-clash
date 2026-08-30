import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  buildConnectionGroups,
  shortProcessName,
  summarizeConnections,
} from './connection-stats.ts'

interface ConnectionSeed {
  id: string
  process?: string
  processPath?: string
  chains?: string[]
  download?: number
  upload?: number
  curDownload?: number
  curUpload?: number
}

const connection = (seed: ConnectionSeed): IConnectionsItem => ({
  id: seed.id,
  metadata: {
    network: 'tcp',
    type: 'HTTPS',
    host: `${seed.id}.example`,
    sourceIP: '127.0.0.1',
    sourcePort: '1000',
    destinationPort: '443',
    destinationIP: '10.0.0.1',
    remoteDestination: '',
    process: seed.process ?? '',
    processPath: seed.processPath ?? '',
  },
  upload: seed.upload ?? 0,
  download: seed.download ?? 0,
  start: '2026-08-30T10:00:00.000Z',
  chains: seed.chains ?? ['DIRECT'],
  rule: 'Match',
  rulePayload: '',
  curUpload: seed.curUpload ?? 0,
  curDownload: seed.curDownload ?? 0,
})

const byProcess = (item: IConnectionsItem) => item.metadata.process ?? ''

describe('shortProcessName', () => {
  it('оставляет только имя файла от полного пути', () => {
    assert.equal(shortProcessName('C:\\Program Files\\app.exe'), 'app.exe')
    assert.equal(shortProcessName('/usr/bin/curl'), 'curl')
    assert.equal(shortProcessName('chrome.exe'), 'chrome.exe')
    assert.equal(shortProcessName(''), '')
  })
})

describe('buildConnectionGroups', () => {
  const rows = [
    connection({ id: 'a1', process: 'a.exe', download: 100, upload: 10 }),
    connection({ id: 'b1', process: 'b.exe', download: 700, upload: 20 }),
    connection({ id: 'a2', process: 'a.exe', download: 300, upload: 40 }),
    connection({ id: 'x1', download: 5, upload: 5 }),
  ]

  it('складывает трафик группы и сохраняет её строки', () => {
    const groups = buildConnectionGroups(rows, byProcess, 'Прочее')
    const a = groups.find((group) => group.key === 'a.exe')

    assert.ok(a)
    assert.equal(a.rows.length, 2)
    assert.deepEqual(
      a.rows.map((row) => row.id),
      ['a1', 'a2'],
    )
    assert.equal(a.download, 400)
    assert.equal(a.upload, 50)
  })

  it('ставит наверх того, кто съел больше всех', () => {
    const groups = buildConnectionGroups(rows, byProcess, 'Прочее')
    assert.deepEqual(
      groups.map((group) => group.key),
      ['b.exe', 'a.exe', 'Прочее'],
    )
  })

  it('собирает соединения без ключа в запасную группу', () => {
    const groups = buildConnectionGroups(rows, byProcess, 'Прочее')
    const other = groups.find((group) => group.key === 'Прочее')

    assert.ok(other)
    assert.deepEqual(
      other.rows.map((row) => row.id),
      ['x1'],
    )
  })

  it('не теряет ни одного соединения', () => {
    const groups = buildConnectionGroups(rows, byProcess, 'Прочее')
    const total = groups.reduce((sum, group) => sum + group.rows.length, 0)

    assert.equal(total, rows.length)
  })

  it('на пустом списке отдаёт пустой результат', () => {
    assert.deepEqual(buildConnectionGroups([], byProcess, 'Прочее'), [])
  })

  it('суммирует скорость группы', () => {
    const groups = buildConnectionGroups(
      [
        connection({
          id: 'c1',
          process: 'c.exe',
          curDownload: 10,
          curUpload: 1,
        }),
        connection({
          id: 'c2',
          process: 'c.exe',
          curDownload: 5,
          curUpload: 2,
        }),
      ],
      byProcess,
      'Прочее',
    )

    assert.equal(groups[0].downloadSpeed, 15)
    assert.equal(groups[0].uploadSpeed, 3)
  })
})

describe('summarizeConnections', () => {
  const labels = { noProcess: 'Без процесса', direct: 'Напрямую' }
  const rows = [
    connection({
      id: '1',
      process: 'chrome.exe',
      chains: ['NL-01', 'Прокси'],
      download: 1000,
      upload: 100,
      curDownload: 50,
      curUpload: 5,
    }),
    connection({
      id: '2',
      processPath: '/opt/steam/steam.exe',
      chains: ['DIRECT'],
      download: 4000,
      upload: 40,
      curDownload: 200,
      curUpload: 2,
    }),
    connection({
      id: '3',
      process: 'chrome.exe',
      chains: ['NL-01', 'Прокси'],
      download: 500,
      upload: 50,
    }),
    connection({ id: '4', chains: [], download: 7, upload: 3 }),
  ]

  it('считает итоги и текущую скорость по показанным строкам', () => {
    const stats = summarizeConnections(rows, labels)

    assert.equal(stats.download, 5507)
    assert.equal(stats.upload, 193)
    assert.equal(stats.downloadSpeed, 250)
    assert.equal(stats.uploadSpeed, 7)
  })

  it('сводит приложения по короткому имени и считает их число', () => {
    const stats = summarizeConnections(rows, labels)

    assert.equal(stats.processCount, 3)
    assert.deepEqual(
      stats.processes.map((entry) => [entry.key, entry.value]),
      [
        ['steam.exe', 4040],
        ['chrome.exe', 1650],
        ['Без процесса', 10],
      ],
    )
  })

  it('называет прямой выход по-человечески и берёт реальный узел цепочки', () => {
    const stats = summarizeConnections(rows, labels)

    assert.deepEqual(
      stats.routes.map((entry) => entry.key),
      ['Напрямую', 'NL-01', 'Без процесса'],
    )
  })

  it('оставляет не больше четырёх строк в каждом списке', () => {
    const many = Array.from({ length: 9 }, (_, index) =>
      connection({
        id: `m${index}`,
        process: `p${index}.exe`,
        chains: [`node-${index}`],
        download: index * 10,
      }),
    )
    const stats = summarizeConnections(many, labels)

    assert.equal(stats.processes.length, 4)
    assert.equal(stats.routes.length, 4)
    assert.equal(stats.processes[0].key, 'p8.exe')
    assert.equal(stats.processCount, 9)
  })

  it('на пустом списке отдаёт нули', () => {
    const stats = summarizeConnections([], labels)

    assert.equal(stats.download, 0)
    assert.equal(stats.processCount, 0)
    assert.deepEqual(stats.processes, [])
    assert.deepEqual(stats.routes, [])
  })
})
