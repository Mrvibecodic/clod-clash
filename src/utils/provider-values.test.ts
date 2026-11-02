import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  lineLabel,
  type ProviderValuesGroupView,
  seenLines,
  showsTemplateValue,
  valueText,
} from './provider-values.ts'

test('подпись строки — слово настроек, а у ключей DNS — сам ключ', () => {
  assert.deepEqual(lineLabel('log-level'), {
    i18n: 'settings.sections.clash.form.fields.logLevel',
  })
  assert.deepEqual(lineLabel('dns.enhanced-mode'), {
    text: 'dns.enhanced-mode',
  })
  assert.deepEqual(lineLabel('hosts'), { text: 'hosts' })
})

test('значения строк — словами окна, а не сырым YAML', () => {
  assert.deepEqual(valueText('mode', 'global'), {
    kind: 'i18n',
    key: 'proxies.page.modes.global',
  })
  assert.deepEqual(valueText('mode', 'script'), {
    kind: 'text',
    text: 'script',
  })
  assert.deepEqual(valueText('tun.stack', 'gvisor'), {
    kind: 'text',
    text: 'gVisor',
  })
  assert.deepEqual(valueText('unified-delay', false), {
    kind: 'i18n',
    key: 'shared.options.off',
  })
  assert.deepEqual(valueText('tun.dns-hijack', ['any:53', 'tcp://any:53']), {
    kind: 'text',
    text: 'any:53, tcp://any:53',
  })
  assert.deepEqual(valueText('tunnels', [{ network: ['tcp'] }]), {
    kind: 'text',
    text: '1',
  })
  assert.deepEqual(
    valueText('tunnels', ['tcp/udp,0.0.0.0:6553,example.com:53,PROXY']),
    { kind: 'text', text: '1' },
  )
  assert.deepEqual(valueText('hosts', { 'a.example.com': '10.0.0.1' }), {
    kind: 'i18n',
    key: 'shared.providerValues.values.entries',
    values: { count: 1 },
  })
  assert.deepEqual(valueText('mixed-port', 7890), {
    kind: 'text',
    text: '7890',
  })
  assert.deepEqual(valueText('rules', { own: 12, hidden: 1 }), {
    kind: 'i18n',
    key: 'shared.providerValues.values.seq',
    values: { own: 12, hidden: 1 },
  })
  assert.deepEqual(valueText('merge', 3), {
    kind: 'i18n',
    key: 'shared.providerValues.values.keys',
    values: { count: 3 },
  })
  assert.deepEqual(valueText('script', true), {
    kind: 'i18n',
    key: 'shared.providerValues.values.own',
  })
  assert.deepEqual(valueText('tun.mtu', null), { kind: 'text', text: '' })
})

test('бэкенду уходит ровно то, что человек видел в отмеченных группах', () => {
  const groups: ProviderValuesGroupView[] = [
    {
      group: 'logs',
      shared: true,
      lines: [{ key: 'log-level', now: 'debug', template: 'info' }],
    },
    {
      group: 'edits',
      shared: false,
      lines: [{ key: 'merge', now: 2, template: null, edited: true }],
    },
  ]
  assert.deepEqual(seenLines(groups, new Set(['logs'])), [
    {
      group: 'logs',
      key: 'log-level',
      now: 'debug',
      template: 'info',
      edited: false,
    },
  ])
  assert.deepEqual(seenLines(groups, new Set(['edits'])), [
    { group: 'edits', key: 'merge', now: 2, template: null, edited: true },
  ])
  assert.deepEqual(seenLines(groups, new Set()), [])
})

test('у туннелей и своих правок «станет» — без значения шаблона', () => {
  assert.equal(showsTemplateValue('log-level'), true)
  assert.equal(showsTemplateValue('tunnels'), false)
  assert.equal(showsTemplateValue('merge'), false)
})
