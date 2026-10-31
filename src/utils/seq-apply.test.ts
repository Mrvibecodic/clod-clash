import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  applySeq,
  groupPolicyOptions,
  groupsEditorSource,
  seqDeleteNames,
} from './seq-apply.ts'

const group = (name: string) => ({ name, type: 'select', proxies: [] })

describe('правки списков подписки', () => {
  it('удалённое объектом { name } убирается так же, как строкой', () => {
    const origin = [group('A'), group('B'), group('C')]
    const applied = applySeq(origin, {
      prepend: [group('Новая')],
      delete: [{ name: 'B' }, 'C', { name: 'B' }, 7, null],
      append: [group('В конце')],
    })
    assert.deepEqual(
      applied.map((item) => item.name),
      ['Новая', 'A', 'В конце'],
    )
    assert.deepEqual(seqDeleteNames([{ name: 'B' }, 'C', { name: 'B' }]), [
      'B',
      'C',
    ])
  })

  it('редактор групп: кривые разделы подписки не попадают в списки', () => {
    const source = groupsEditorSource(
      {
        proxies: [{ name: 'узел 1' }, 'узел 2', { name: 'убран' }, { port: 1 }],
        'proxy-groups': 'не список',
        'proxy-providers': 'не словарь',
      },
      { 'proxy-providers': { свой: {} } },
      { prepend: 'не список', delete: [{ name: 'убран' }], append: ['узел 3'] },
    )
    assert.deepEqual(source.groups, [])
    assert.deepEqual(source.providers, ['свой'])
    assert.deepEqual(source.proxyNames, ['узел 1', 'узел 2', 'узел 3'])

    const full = groupsEditorSource(
      {
        proxies: [{ name: 'узел 1' }],
        'proxy-groups': [group('Прокси'), group('Авто')],
        'proxy-providers': { общий: {} },
      },
      null,
      undefined,
    )
    assert.deepEqual(
      groupPolicyOptions({
        builtin: ['DIRECT', 'REJECT'],
        prepend: [group('Своя')],
        origin: full.groups,
        deleted: ['Авто'],
        append: [group('Прокси')],
        proxies: full.proxyNames,
      }),
      ['DIRECT', 'REJECT', 'Своя', 'Прокси', 'узел 1'],
    )
    assert.deepEqual(full.providers, ['общий'])
  })
})
