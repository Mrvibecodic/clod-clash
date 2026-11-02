import { stackLabel } from './tun-window.ts'

export type ProviderValuesGroup =
  | 'mode'
  | 'logs'
  | 'ports'
  | 'tun'
  | 'tunnels'
  | 'dns'
  | 'edits'

export interface ProviderValuesLine {
  key: string
  now: unknown
  template: unknown
  edited?: boolean
}

export interface ProviderValuesGroupView {
  group: ProviderValuesGroup
  shared: boolean
  lines: ProviderValuesLine[]
}

export interface ProviderValuesView {
  uid: string
  name: string
  groups: ProviderValuesGroupView[]
}

export type ProviderValuesOutcome = 'returned' | 'stale'

export interface SeenLine {
  group: ProviderValuesGroup
  key: string
  now: unknown
  template: unknown
  edited: boolean
}

export const seenLines = (
  groups: ProviderValuesGroupView[],
  selected: ReadonlySet<ProviderValuesGroup>,
): SeenLine[] =>
  groups
    .filter((view) => selected.has(view.group))
    .flatMap((view) =>
      view.lines.map((line) => ({
        group: view.group,
        key: line.key,
        now: line.now,
        template: line.template,
        edited: line.edited ?? false,
      })),
    )

const LABELS: Record<string, string> = {
  mode: 'settings.sections.clash.form.fields.routingMode',
  'log-level': 'settings.sections.clash.form.fields.logLevel',
  'unified-delay': 'settings.sections.clash.form.fields.unifiedDelay',
  'mixed-port': 'settings.modals.clashPort.fields.mixed',
  'allow-lan': 'settings.sections.clash.form.fields.allowLan',
  'tun.stack': 'settings.modals.tun.fields.stack',
  'tun.strict-route': 'settings.modals.tun.fields.strictRoute',
  'tun.dns-hijack': 'settings.modals.tun.fields.dnsHijack',
  'tun.device': 'settings.modals.tun.fields.device',
  'tun.mtu': 'settings.modals.tun.fields.mtu',
  'tun.route-exclude-address': 'settings.modals.tun.fields.routeExcludeAddress',
  tunnels: 'shared.providerValues.groups.tunnels',
  rules: 'shared.providerValues.edits.rules',
  proxies: 'shared.providerValues.edits.proxies',
  groups: 'shared.providerValues.edits.groups',
  merge: 'shared.providerValues.edits.merge',
  script: 'shared.providerValues.edits.script',
}

export const lineLabel = (key: string): { i18n: string } | { text: string } =>
  key in LABELS ? { i18n: LABELS[key] } : { text: key }

const EDIT_KEYS = ['rules', 'proxies', 'groups', 'merge', 'script']

export const showsTemplateValue = (key: string) =>
  key !== 'tunnels' && !EDIT_KEYS.includes(key)

export type ValueText =
  | { kind: 'text'; text: string }
  | { kind: 'i18n'; key: string; values?: Record<string, unknown> }

const MODES = ['rule', 'global', 'direct']

export const valueText = (key: string, value: unknown): ValueText => {
  if (key === 'tunnels' && Array.isArray(value)) {
    return { kind: 'text', text: String(value.length) }
  }
  if (key === 'tun.stack' && typeof value === 'string') {
    return { kind: 'text', text: stackLabel(value) }
  }
  if (key === 'mode' && typeof value === 'string' && MODES.includes(value)) {
    return { kind: 'i18n', key: `proxies.page.modes.${value}` }
  }
  if (EDIT_KEYS.includes(key)) {
    if (value !== null && typeof value === 'object') {
      const { own = 0, hidden = 0 } = value as Record<string, number>
      return {
        kind: 'i18n',
        key: 'shared.providerValues.values.seq',
        values: { own, hidden },
      }
    }
    if (typeof value === 'number') {
      return {
        kind: 'i18n',
        key: 'shared.providerValues.values.keys',
        values: { count: value },
      }
    }
    return { kind: 'i18n', key: 'shared.providerValues.values.own' }
  }
  if (typeof value === 'boolean') {
    return {
      kind: 'i18n',
      key: value ? 'shared.options.on' : 'shared.options.off',
    }
  }
  if (Array.isArray(value)) {
    return {
      kind: 'text',
      text: value.every((item) => typeof item !== 'object')
        ? value.join(', ')
        : String(value.length),
    }
  }
  if (value !== null && typeof value === 'object') {
    return {
      kind: 'i18n',
      key: 'shared.providerValues.values.entries',
      values: { count: Object.keys(value).length },
    }
  }
  return { kind: 'text', text: value == null ? '' : String(value) }
}
