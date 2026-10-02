import { RestartAltRounded } from '@mui/icons-material'
import {
  Box,
  Button,
  MenuItem,
  Select,
  Tab,
  Tabs,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from '@mui/material'
import { invoke } from '@tauri-apps/api/core'
import { useLockFn } from 'ahooks'
import yaml from 'js-yaml'
import type { Ref } from 'react'
import {
  useCallback,
  useEffect,
  useImperativeHandle,
  useReducer,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  type DialogRef,
  MonacoEditor,
  Switch,
} from '@/components/base'
import { useClash } from '@/hooks/use-clash'
import { useProfiles } from '@/hooks/use-profiles'
import { useVerge } from '@/hooks/use-verge'
import { showNotice } from '@/services/notice-service'
import { useThemeMode } from '@/services/states'
import type { MonacoEditorInstance } from '@/types/monaco'
import {
  asDnsMapping,
  listenFieldFrom,
  mergeDnsConfig,
  readDnsBlock,
  summarizeValidation,
} from '@/utils/dns-config'
import getSystem from '@/utils/get-system'

type NameserverPolicy = Record<string, any>

function parseNameserverPolicy(str: string): NameserverPolicy {
  const result: NameserverPolicy = {}
  if (!str) return result

  const ruleRegex = /\s*([^=]+?)\s*=\s*([^,]+)(?:,|$)/g
  let match: RegExpExecArray | null

  while ((match = ruleRegex.exec(str)) !== null) {
    const [, domainsPart, serversPart] = match

    const domains = [domainsPart.trim()]
    const servers = serversPart.split(';').map((s) => s.trim())

    domains.forEach((domain) => {
      result[domain] = servers
    })
  }

  return result
}

function formatNameserverPolicy(policy: unknown): string {
  if (!policy || typeof policy !== 'object') return ''

  return Object.entries(policy as Record<string, unknown>)
    .map(([domain, servers]) => {
      const serversStr = Array.isArray(servers) ? servers.join(';') : servers
      return `${domain}=${serversStr}`
    })
    .join(', ')
}

function formatHosts(hosts: unknown): string {
  if (!hosts || typeof hosts !== 'object') return ''

  const result: string[] = []

  Object.entries(hosts as Record<string, unknown>).forEach(
    ([domain, value]) => {
      if (Array.isArray(value)) {
        const ipsStr = value.join(';')
        result.push(`${domain}=${ipsStr}`)
      } else {
        result.push(`${domain}=${value}`)
      }
    },
  )

  return result.join(', ')
}

function parseHosts(str: string): NameserverPolicy {
  const result: NameserverPolicy = {}
  if (!str) return result

  str.split(',').forEach((item) => {
    const parts = item.trim().split('=')
    if (parts.length < 2) return

    const domain = parts[0].trim()
    const valueStr = parts.slice(1).join('=').trim()

    if (valueStr.includes(';')) {
      result[domain] = valueStr
        .split(';')
        .map((s) => s.trim())
        .filter(Boolean)
    } else {
      result[domain] = valueStr
    }
  })

  return result
}

function parseList(str: string): string[] {
  if (!str?.trim()) return []
  // Поле многострочное: Enter — тоже разделитель, а не часть записи.
  return str
    .split(/[,\n]/)
    .map((item) => item.trim())
    .filter(Boolean)
}

// clod:hosts-ladder — у ключей hosts три состояния: «как в подписке» (ключа у
// нас нет и решает шаблон провайдера, а если и он молчит — умолчание ядра),
// либо явное включение или выключение поверх подписки.
type HostsChoice = 'auto' | 'on' | 'off'

const hostsChoiceOf = (value: boolean | undefined): HostsChoice =>
  value === undefined ? 'auto' : value ? 'on' : 'off'

const hostsKey = (key: string, choice: HostsChoice) =>
  choice === 'auto' ? {} : { [key]: choice === 'on' }

// clod:dns-page-diff — чем поле показывается, когда ключа нет ни у подписки,
// ни на странице: умолчаниями самого ядра, а не нашими. Поле, оставленное в
// этом положении, в файл не пишется — ключ остаётся за подпиской.
const CORE_DEFAULTS = {
  enable: false,
  'enhanced-mode': 'redir-host' as 'fake-ip' | 'redir-host',
  'fake-ip-range': '198.18.0.1/16',
  'fake-ip-range6': '',
  'fake-ip-filter-mode': 'blacklist' as 'blacklist' | 'whitelist',
  'prefer-h3': false,
  'respect-rules': false,
  ipv6: false,
  'fake-ip-filter': [] as string[],
  'default-nameserver': [] as string[],
  nameserver: [] as string[],
  'nameserver-policy': {} as Record<string, unknown>,
  'proxy-server-nameserver': [] as string[],
  'direct-nameserver': [] as string[],
  'direct-nameserver-follow-policy': false,
}

// Поле формы → ключ блока dns. Нетронутое поле не пишется по значению формы:
// ключа не было — его не будет, был — уходит как был (форма режет списки по
// запятым и не знает всех режимов ядра, а перепечатывать чужое нельзя).
const FIELD_KEYS = {
  enable: 'enable',
  listen: 'listen',
  enhancedMode: 'enhanced-mode',
  fakeIpRange: 'fake-ip-range',
  fakeIpRange6: 'fake-ip-range6',
  fakeIpFilterMode: 'fake-ip-filter-mode',
  preferH3: 'prefer-h3',
  respectRules: 'respect-rules',
  useHosts: 'use-hosts',
  useSystemHosts: 'use-system-hosts',
  ipv6: 'ipv6',
  fakeIpFilter: 'fake-ip-filter',
  defaultNameserver: 'default-nameserver',
  nameserver: 'nameserver',
  proxyServerNameserver: 'proxy-server-nameserver',
  directNameserver: 'direct-nameserver',
  directNameserverFollowPolicy: 'direct-nameserver-follow-policy',
  nameserverPolicy: 'nameserver-policy',
} as const

interface DnsValues {
  enable: boolean
  listen: string
  enhancedMode: 'fake-ip' | 'redir-host'
  fakeIpRange: string
  fakeIpRange6: string
  fakeIpFilterMode: 'blacklist' | 'whitelist'
  preferH3: boolean
  respectRules: boolean
  useHosts: HostsChoice
  useSystemHosts: HostsChoice
  ipv6: boolean
  fakeIpFilter: string
  nameserver: string
  defaultNameserver: string
  proxyServerNameserver: string
  directNameserver: string
  directNameserverFollowPolicy: boolean
  nameserverPolicy: string
  hosts: string
}

type DnsRow = { field: keyof DnsValues; label: string; hint?: string } & (
  | { kind: 'switch' }
  | { kind: 'select'; options: [string, string][] }
  | { kind: 'text' | 'list'; placeholder?: string }
)

// Вкладки окна: каждая помещается без прокрутки и в окне простого режима.
interface DnsTab {
  id: 'main' | 'fakeIp' | 'servers' | 'hosts'
  title: string
  sections: { title?: string; rows: DnsRow[] }[]
}

const dnsTabs = (t: ReturnType<typeof useTranslation>['t']): DnsTab[] => {
  const hostsOptions: [string, string][] = [
    ['auto', t('settings.modals.dns.options.hosts.auto')],
    ['on', t('settings.modals.dns.options.hosts.on')],
    ['off', t('settings.modals.dns.options.hosts.off')],
  ]
  return [
    {
      id: 'main',
      title: t('settings.modals.dns.tabs.main'),
      sections: [
        {
          rows: [
            {
              kind: 'switch',
              field: 'enable',
              label: t('settings.modals.dns.fields.enable'),
              hint: t('settings.modals.dns.fields.enableHint'),
            },
            {
              kind: 'select',
              field: 'enhancedMode',
              label: t('settings.modals.dns.fields.enhancedMode'),
              hint: t('settings.modals.dns.fields.enhancedModeHint'),
              options: [
                ['fake-ip', 'fake-ip'],
                ['redir-host', 'redir-host'],
              ],
            },
            {
              kind: 'text',
              field: 'listen',
              label: t('settings.modals.dns.fields.listen'),
              hint: t('settings.modals.dns.fields.listenHint'),
              placeholder: '127.0.0.1:1053',
            },
            {
              kind: 'switch',
              field: 'ipv6',
              label: t('settings.modals.dns.fields.ipv6.label'),
              hint: t('settings.modals.dns.fields.ipv6.description'),
            },
          ],
        },
        {
          title: t('settings.modals.dns.sections.behavior'),
          rows: [
            {
              kind: 'switch',
              field: 'respectRules',
              label: t('settings.modals.dns.fields.respectRules.label'),
            },
            {
              kind: 'switch',
              field: 'preferH3',
              label: t('settings.modals.dns.fields.preferH3.label'),
            },
            {
              kind: 'select',
              field: 'useHosts',
              label: t('settings.modals.dns.fields.useHosts.label'),
              options: hostsOptions,
            },
            {
              kind: 'select',
              field: 'useSystemHosts',
              label: t('settings.modals.dns.fields.useSystemHosts.label'),
              options: hostsOptions,
            },
          ],
        },
      ],
    },
    {
      id: 'fakeIp',
      title: 'FakeIP',
      sections: [
        {
          rows: [
            {
              kind: 'text',
              field: 'fakeIpRange',
              label: t('settings.modals.dns.fields.fakeIpRange'),
              placeholder: '198.18.0.1/16',
            },
            {
              kind: 'text',
              field: 'fakeIpRange6',
              label: t('settings.modals.dns.fields.fakeIpRange6'),
              placeholder: '2001:2::0/64',
            },
            {
              kind: 'select',
              field: 'fakeIpFilterMode',
              label: t('settings.modals.dns.fields.fakeIpFilterMode'),
              options: [
                [
                  'blacklist',
                  t('settings.modals.dns.options.filterMode.blacklist'),
                ],
                [
                  'whitelist',
                  t('settings.modals.dns.options.filterMode.whitelist'),
                ],
              ],
            },
            {
              kind: 'list',
              field: 'fakeIpFilter',
              label: t('settings.modals.dns.fields.fakeIpFilter.label'),
              placeholder: '*.lan, *.local, localhost',
            },
          ],
        },
      ],
    },
    {
      id: 'servers',
      title: t('settings.modals.dns.tabs.servers'),
      sections: [
        {
          rows: [
            {
              kind: 'list',
              field: 'nameserver',
              label: t('settings.modals.dns.fields.nameserver.label'),
              placeholder: 'https://1.1.1.1/dns-query',
            },
            {
              kind: 'list',
              field: 'defaultNameserver',
              label: t('settings.modals.dns.fields.defaultNameserver.label'),
              hint: t(
                'settings.modals.dns.fields.defaultNameserver.description',
              ),
              placeholder: '9.9.9.9, 1.1.1.1',
            },
            {
              kind: 'list',
              field: 'proxyServerNameserver',
              label: t('settings.modals.dns.fields.proxy.label'),
              placeholder: 'https://1.1.1.1/dns-query',
            },
            {
              kind: 'list',
              field: 'directNameserver',
              label: t('settings.modals.dns.fields.directNameserver.label'),
              hint: t(
                'settings.modals.dns.fields.directNameserver.description',
              ),
              placeholder: 'system',
            },
            {
              kind: 'switch',
              field: 'directNameserverFollowPolicy',
              label: t('settings.modals.dns.fields.directPolicy.label'),
            },
            {
              kind: 'list',
              field: 'nameserverPolicy',
              label: t('settings.modals.dns.fields.nameserverPolicy.label'),
              hint: t(
                'settings.modals.dns.fields.nameserverPolicy.description',
              ),
              placeholder: '+.example.com=https://1.1.1.1/dns-query',
            },
          ],
        },
      ],
    },
    {
      id: 'hosts',
      title: 'Hosts',
      sections: [
        {
          rows: [
            {
              kind: 'list',
              field: 'hosts',
              label: t('settings.modals.dns.fields.hosts.label'),
              hint: t('settings.modals.dns.fields.hosts.description'),
              placeholder: 'router.lan=192.168.1.1',
            },
          ],
        },
      ],
    },
  ]
}

// Ширина полей справа: одна на все строки, в узком окне простого режима
// уступает подписи.
const CONTROL_SX = { width: 'clamp(140px, 38%, 190px)', flexShrink: 0 }

export function DnsViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const { mutateClash } = useClash()
  const { verge } = useVerge()
  const { profiles } = useProfiles()
  const themeMode = useThemeMode()

  const [open, setOpen] = useState(false)
  const [visualization, setVisualization] = useState(true)
  const skipYamlSyncRef = useRef(false)
  const [seeding, setSeeding] = useState(false)
  const parsedDnsRef = useRef<unknown>({})
  const touchedRef = useRef(new Set<string>())
  const baseHostsRef = useRef<unknown>(undefined)
  const renderedTextRef = useRef({ nameserverPolicy: '', hosts: '' })
  const editorRef = useRef<MonacoEditorInstance | null>(null)
  const [tab, setTab] = useState(0)
  const [values, setValues] = useState<DnsValues>({
    enable: CORE_DEFAULTS.enable,
    listen: '',
    enhancedMode: CORE_DEFAULTS['enhanced-mode'],
    fakeIpRange: CORE_DEFAULTS['fake-ip-range'],
    fakeIpRange6: CORE_DEFAULTS['fake-ip-range6'],
    fakeIpFilterMode: CORE_DEFAULTS['fake-ip-filter-mode'],
    preferH3: CORE_DEFAULTS['prefer-h3'],
    respectRules: CORE_DEFAULTS['respect-rules'],
    useHosts: 'auto',
    useSystemHosts: 'auto',
    ipv6: CORE_DEFAULTS.ipv6,
    fakeIpFilter: CORE_DEFAULTS['fake-ip-filter'].join(', '),
    defaultNameserver: CORE_DEFAULTS['default-nameserver'].join(', '),
    nameserver: CORE_DEFAULTS.nameserver.join(', '),
    proxyServerNameserver:
      CORE_DEFAULTS['proxy-server-nameserver']?.join(', ') || '',
    directNameserver: CORE_DEFAULTS['direct-nameserver']?.join(', ') || '',
    directNameserverFollowPolicy:
      CORE_DEFAULTS['direct-nameserver-follow-policy'] || false,
    nameserverPolicy: '',
    hosts: '',
  })

  const [yamlContent, setYamlContent] = useReducer(
    (_: string, next: string) => next,
    '',
  )

  const updateValuesFromConfig = useCallback(
    (config: any) => {
      if (!config) return

      const dnsConfig: any = readDnsBlock(config)
      const hostsConfig = config.hosts || {}

      parsedDnsRef.current = dnsConfig
      baseHostsRef.current = config.hosts
      touchedRef.current = new Set()

      const nameserverPolicyText =
        formatNameserverPolicy(dnsConfig['nameserver-policy']) || ''
      const hostsText = formatHosts(hostsConfig) || ''
      renderedTextRef.current = {
        nameserverPolicy: nameserverPolicyText,
        hosts: hostsText,
      }

      const enhancedMode =
        dnsConfig['enhanced-mode'] || CORE_DEFAULTS['enhanced-mode']
      const validEnhancedMode =
        enhancedMode === 'fake-ip' || enhancedMode === 'redir-host'
          ? enhancedMode
          : CORE_DEFAULTS['enhanced-mode']

      const fakeIpFilterMode =
        dnsConfig['fake-ip-filter-mode'] || CORE_DEFAULTS['fake-ip-filter-mode']
      const validFakeIpFilterMode =
        fakeIpFilterMode === 'blacklist' || fakeIpFilterMode === 'whitelist'
          ? fakeIpFilterMode
          : CORE_DEFAULTS['fake-ip-filter-mode']

      setValues({
        enable: dnsConfig.enable ?? CORE_DEFAULTS.enable,
        listen: listenFieldFrom(dnsConfig.listen),
        enhancedMode: validEnhancedMode,
        fakeIpRange:
          dnsConfig['fake-ip-range'] ?? CORE_DEFAULTS['fake-ip-range'],
        fakeIpRange6:
          dnsConfig['fake-ip-range6'] ?? CORE_DEFAULTS['fake-ip-range6'],
        fakeIpFilterMode: validFakeIpFilterMode,
        preferH3: dnsConfig['prefer-h3'] ?? CORE_DEFAULTS['prefer-h3'],
        respectRules:
          dnsConfig['respect-rules'] ?? CORE_DEFAULTS['respect-rules'],
        useHosts: hostsChoiceOf(dnsConfig['use-hosts']),
        useSystemHosts: hostsChoiceOf(dnsConfig['use-system-hosts']),
        ipv6: dnsConfig.ipv6 ?? CORE_DEFAULTS.ipv6,
        fakeIpFilter:
          dnsConfig['fake-ip-filter']?.join(', ') ??
          CORE_DEFAULTS['fake-ip-filter'].join(', '),
        nameserver:
          dnsConfig.nameserver?.join(', ') ??
          CORE_DEFAULTS.nameserver.join(', '),
        defaultNameserver:
          dnsConfig['default-nameserver']?.join(', ') ??
          CORE_DEFAULTS['default-nameserver'].join(', '),
        proxyServerNameserver:
          dnsConfig['proxy-server-nameserver']?.join(', ') ??
          (CORE_DEFAULTS['proxy-server-nameserver']?.join(', ') || ''),
        directNameserver:
          dnsConfig['direct-nameserver']?.join(', ') ??
          (CORE_DEFAULTS['direct-nameserver']?.join(', ') || ''),
        directNameserverFollowPolicy:
          dnsConfig['direct-nameserver-follow-policy'] ??
          CORE_DEFAULTS['direct-nameserver-follow-policy'],
        nameserverPolicy: nameserverPolicyText,
        hosts: hostsText,
      })
    },
    [setValues],
  )

  const generateDnsConfig = useCallback(() => {
    let formFields: Record<string, any> = {
      enable: values.enable,
      ...(values.listen.trim() ? { listen: values.listen.trim() } : {}),
      'enhanced-mode': values.enhancedMode,
      // clod:dns-page-fits — пустой диапазон ядро принимает молча и остаётся
      // без пула: fake-ip перестаёт работать, а сказать об этом некому. Пустое
      // поле означает «как по умолчанию». Пустой диапазон IPv6 — ключа нет:
      // при включённом ipv6 бэкенд подставит свой.
      'fake-ip-range': values.fakeIpRange || CORE_DEFAULTS['fake-ip-range'],
      ...(values.fakeIpRange6.trim()
        ? { 'fake-ip-range6': values.fakeIpRange6.trim() }
        : {}),
      'fake-ip-filter-mode': values.fakeIpFilterMode,
      'prefer-h3': values.preferH3,
      'respect-rules': values.respectRules,
      ...hostsKey('use-hosts', values.useHosts),
      ...hostsKey('use-system-hosts', values.useSystemHosts),
      ipv6: values.ipv6,
      // Пустой список — значение, а не молчание: в чёрном списке он означает
      // «исключений нет», в белом — «fake-ip выключен». Подменять написанное
      // человеком нашим списком нельзя. Список, которого не было ни у
      // подписки, ни на странице, и который остался пустым, ниже выбрасывается.
      'fake-ip-filter': parseList(values.fakeIpFilter),
      'default-nameserver': parseList(values.defaultNameserver),
      nameserver: parseList(values.nameserver),
      'direct-nameserver-follow-policy': values.directNameserverFollowPolicy,
      'proxy-server-nameserver': parseList(values.proxyServerNameserver),
      'direct-nameserver': parseList(values.directNameserver),
    }

    const basePolicy = asDnsMapping(parsedDnsRef.current)?.['nameserver-policy']
    if (
      values.nameserverPolicy === renderedTextRef.current.nameserverPolicy &&
      basePolicy !== undefined
    ) {
      formFields['nameserver-policy'] = basePolicy
    } else {
      const policy = parseNameserverPolicy(values.nameserverPolicy)
      if (Object.keys(policy).length > 0) {
        formFields['nameserver-policy'] = policy
      }
    }

    // Нетронутые поля: ключ был — остаётся как был, не было — не появляется.
    const shown = asDnsMapping(parsedDnsRef.current) ?? {}
    for (const [field, key] of Object.entries(FIELD_KEYS)) {
      if (touchedRef.current.has(field)) continue
      if (key in shown) {
        formFields[key] = shown[key]
      } else {
        formFields = Object.fromEntries(
          Object.entries(formFields).filter(([name]) => name !== key),
        )
      }
    }

    return mergeDnsConfig(parsedDnsRef.current, formFields)
  }, [values])

  const generateHostsConfig = useCallback(() => {
    if (
      values.hosts === renderedTextRef.current.hosts &&
      baseHostsRef.current !== undefined
    ) {
      return baseHostsRef.current
    }

    return parseHosts(values.hosts)
  }, [values.hosts])

  const updateYamlFromValues = useCallback(() => {
    const config: Record<string, any> = {}

    const dnsConfig = generateDnsConfig()
    if (Object.keys(dnsConfig).length > 0) {
      config.dns = dnsConfig
    }

    const hosts = generateHostsConfig()
    if (Object.keys(asDnsMapping(hosts) ?? {}).length > 0) {
      config.hosts = hosts
    }

    setYamlContent(yaml.dump(config, { forceQuotes: true }))
  }, [generateDnsConfig, generateHostsConfig, setYamlContent])

  // clod:dns-page-diff — страница хранит только отличия от подписки, поэтому
  // «сброс» здесь один: показать подписку как есть. Сохранение после него
  // оставит страницу пустой.
  const showTheSubscription = useCallback(async () => {
    try {
      const view = await invoke<Record<string, unknown>>('get_dns_page_view', {
        bare: true,
      })
      skipYamlSyncRef.current = true
      updateValuesFromConfig(view)
      setYamlContent(yaml.dump(view, { forceQuotes: true }))
    } catch (err) {
      showNotice.error(err)
    }
  }, [setYamlContent, updateValuesFromConfig])

  const updateValuesFromYaml = () => {
    let parsedYaml: any
    try {
      parsedYaml = yaml.load(yamlContent)
    } catch {
      parsedYaml = null
    }
    if (!parsedYaml || typeof parsedYaml !== 'object') {
      showNotice.error('settings.modals.dns.errors.invalidYaml')
      return false
    }
    skipYamlSyncRef.current = true
    updateValuesFromConfig(parsedYaml)
    return true
  }

  useEffect(() => {
    if (skipYamlSyncRef.current) {
      skipYamlSyncRef.current = false
      return
    }
    updateYamlFromValues()
  }, [updateYamlFromValues])

  useEffect(() => {
    return () => {
      editorRef.current?.dispose()
      editorRef.current = null
    }
  }, [])

  // Редактор показывает подписку, поверх неё — отличия со страницы; бэкенд
  // при сохранении оставит от присланного ровно отличия.
  const initDnsConfig = useCallback(async () => {
    setSeeding(true)
    try {
      const view = await invoke<Record<string, unknown>>('get_dns_page_view', {
        bare: false,
      })
      skipYamlSyncRef.current = true
      updateValuesFromConfig(view)
      setYamlContent(yaml.dump(view, { forceQuotes: true }))
    } catch (err) {
      console.error('Failed to initialize DNS config', err)
      showNotice.error(err)
      // Прежние значения формы к этой странице не относятся — сохранять их
      // поверх неразобранного файла нельзя.
      setOpen(false)
    } finally {
      setSeeding(false)
    }
  }, [setYamlContent, updateValuesFromConfig])

  // Страница DNS принадлежит подписке: и то, чем диалог засеян, и то, куда
  // уйдёт «Сохранить». Если подписку сменили из трея, пока диалог открыт, эти
  // двое расходятся — тогда правки одной подписки затёрли бы страницу другой.
  // Пустого `current` при перечитывании не бывает, но если запрос всё же
  // отдаст его пустым, закрывать диалог не за что.
  const editedProfileRef = useRef<string | undefined>(undefined)
  if (
    open &&
    profiles?.current &&
    profiles.current !== editedProfileRef.current
  )
    setOpen(false)

  useImperativeHandle(
    ref,
    () => ({
      open: () => {
        editedProfileRef.current = profiles?.current
        setTab(0)
        setOpen(true)
        void initDnsConfig()
      },
      close: () => setOpen(false),
    }),
    [initDnsConfig, profiles],
  )

  const onSave = useLockFn(async () => {
    try {
      let config: Record<string, any>

      if (visualization) {
        config = {}

        const dnsConfig = generateDnsConfig()
        if (Object.keys(dnsConfig).length > 0) {
          config.dns = dnsConfig
        }

        const hosts = generateHostsConfig()
        if (Object.keys(asDnsMapping(hosts) ?? {}).length > 0) {
          config.hosts = hosts
        }
      } else {
        const parsedConfig = yaml.load(yamlContent)
        if (typeof parsedConfig !== 'object' || parsedConfig === null) {
          throw new Error(t('settings.modals.dns.errors.invalid'))
        }
        config = parsedConfig as Record<string, any>
      }

      const outcome = await invoke<DnsSaveOutcome>('save_dns_config', {
        dnsConfig: config,
      })

      if (!outcome.saved) {
        showNotice.error(
          'settings.modals.dns.messages.notSaved',
          summarizeValidation(outcome.validation),
        )
        return
      }

      // Бэкенд сам доставил сборку ядру, если тумблер включён; повторное
      // применение отсюда перезагружало бы ядро второй раз.
      if (verge?.enable_dns_settings) mutateClash()

      setOpen(false)

      if (outcome.warning) {
        showNotice.warning(outcome.warning, 0)
      }
      if (outcome.deliveryError) {
        showNotice.error(
          'settings.modals.dns.messages.savedNotApplied',
          outcome.deliveryError,
        )
      } else if (outcome.validation.status === 'valid') {
        showNotice.success('settings.modals.dns.messages.saved')
      } else {
        showNotice.info(
          'settings.modals.dns.messages.savedUnchecked',
          summarizeValidation(outcome.validation),
        )
      }
    } catch (err) {
      showNotice.error(err)
    }
  })

  const handleYamlChange = (value?: string) => {
    setYamlContent(value || '')
  }

  const handleChange = (field: string) => (event: any) => {
    const value =
      event.target.type === 'checkbox'
        ? event.target.checked
        : event.target.value

    touchedRef.current.add(field)
    setValues((prev) => ({ ...prev, [field]: value }))
  }

  const tabs = dnsTabs(t)
  const currentTab = tabs[tab] ?? tabs[0]
  // Только когда redir-host назван явно: без ключа в TUN бэкенд сам ставит
  // fake-ip, хотя поле показывает умолчание ядра.
  const fakeIpIdle =
    values.enhancedMode === 'redir-host' &&
    (touchedRef.current.has('enhancedMode') ||
      'enhanced-mode' in (asDnsMapping(parsedDnsRef.current) ?? {}))

  const renderRow = (row: DnsRow) => {
    const label = (
      <Box sx={{ minWidth: 0, flex: 1 }}>
        <Typography variant="body2" sx={{ fontWeight: 500 }}>
          {row.label}
        </Typography>
        {row.hint && (
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ display: 'block', lineHeight: 1.35 }}
          >
            {row.hint}
          </Typography>
        )}
      </Box>
    )
    const value = values[row.field]

    if (row.kind === 'list') {
      return (
        <Box key={row.field} sx={{ py: 0.5 }}>
          {label}
          <TextField
            fullWidth
            multiline
            minRows={row.field === 'hosts' ? 8 : 1}
            maxRows={row.field === 'hosts' ? 14 : 4}
            size="small"
            spellCheck="false"
            value={value}
            onChange={handleChange(row.field)}
            placeholder={row.placeholder}
            sx={{ mt: 0.5, '& textarea': { fontSize: 13, lineHeight: 1.5 } }}
          />
        </Box>
      )
    }

    return (
      <Box
        key={row.field}
        sx={{ display: 'flex', alignItems: 'center', gap: 2, py: 0.5 }}
      >
        {label}
        {row.kind === 'switch' && (
          <Switch
            edge="end"
            checked={Boolean(value)}
            onChange={handleChange(row.field)}
          />
        )}
        {row.kind === 'select' && (
          <Select
            size="small"
            sx={{ ...CONTROL_SX, fontSize: 14 }}
            value={value}
            onChange={handleChange(row.field)}
          >
            {row.options.map(([option, text]) => (
              <MenuItem key={option} value={option}>
                {text}
              </MenuItem>
            ))}
          </Select>
        )}
        {row.kind === 'text' && (
          <TextField
            size="small"
            autoComplete="off"
            spellCheck="false"
            value={value}
            onChange={handleChange(row.field)}
            placeholder={row.placeholder}
            sx={{ ...CONTROL_SX, '& input': { fontSize: 14 } }}
          />
        )}
      </Box>
    )
  }

  return (
    <BaseDialog
      open={open}
      disableEnforceFocus={!visualization}
      title={t('settings.modals.dns.dialog.title')}
      contentSx={{
        width: 550,
        overflow: 'auto',
        ...(visualization
          ? {}
          : { padding: '0 24px', display: 'flex', flexDirection: 'column' }),
      }}
      loading={seeding}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <Box
        sx={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          flexWrap: 'wrap',
          gap: 1,
          mb: 1,
        }}
      >
        <ToggleButtonGroup
          size="small"
          exclusive
          value={visualization ? 'form' : 'yaml'}
          onChange={(_, mode) => {
            if (!mode || (mode === 'form') === visualization) return
            if (visualization || updateValuesFromYaml()) {
              setVisualization(!visualization)
            }
          }}
          sx={{
            '& .MuiToggleButton-root': {
              textTransform: 'none',
              px: 1.5,
              py: 0.25,
            },
          }}
        >
          <ToggleButton value="form">
            {t('settings.modals.dns.modes.form')}
          </ToggleButton>
          <ToggleButton value="yaml">YAML</ToggleButton>
        </ToggleButtonGroup>
        <Button
          size="small"
          color="warning"
          startIcon={<RestartAltRounded />}
          disabled={seeding}
          onClick={showTheSubscription}
          sx={{ textTransform: 'none' }}
        >
          {t('settings.modals.dns.actions.asInSubscription')}
        </Button>
      </Box>
      <Typography
        variant="caption"
        color="text.secondary"
        sx={{ display: 'block', mb: 1, lineHeight: 1.4 }}
      >
        {t('settings.modals.dns.dialog.note')}
      </Typography>

      {visualization ? (
        <Box>
          <Tabs
            value={tab}
            onChange={(_, next) => setTab(next)}
            variant="fullWidth"
            sx={{
              minHeight: 36,
              borderBottom: 1,
              borderColor: 'divider',
              mb: 1,
              '& .MuiTab-root': {
                minHeight: 36,
                minWidth: 0,
                px: 1,
                py: 0,
                textTransform: 'none',
              },
            }}
          >
            {tabs.map((item) => (
              <Tab key={item.id} label={item.title} />
            ))}
          </Tabs>
          {/* Высота — по самой длинной вкладке, чтобы окно не прыгало. */}
          <Box sx={{ minHeight: 'min(470px, calc(100vh - 340px))' }}>
            {currentTab.id === 'fakeIp' && fakeIpIdle && (
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ display: 'block', mb: 0.5 }}
              >
                {t('settings.modals.dns.dialog.fakeIpIdle')}
              </Typography>
            )}
            <Box
              sx={
                currentTab.id === 'fakeIp' && fakeIpIdle
                  ? { opacity: 0.55 }
                  : undefined
              }
            >
              {currentTab.sections.map((section) => (
                <Box key={section.title ?? currentTab.id}>
                  {section.title && (
                    <Typography
                      variant="overline"
                      color="text.secondary"
                      sx={{ display: 'block', fontWeight: 700, mt: 1 }}
                    >
                      {section.title}
                    </Typography>
                  )}
                  {section.rows.map(renderRow)}
                </Box>
              ))}
            </Box>
          </Box>
        </Box>
      ) : (
        <MonacoEditor
          height="100vh"
          language="yaml"
          value={yamlContent}
          theme={themeMode === 'light' ? 'light' : 'vs-dark'}
          className="flex-grow"
          onMount={(editorInstance) => {
            editorRef.current = editorInstance
          }}
          options={{
            tabSize: 2,
            minimap: {
              enabled: document.documentElement.clientWidth >= 1500,
            },
            mouseWheelZoom: true,
            quickSuggestions: {
              strings: true,
              comments: true,
              other: true,
            },
            padding: {
              top: 8,
            },
            fontFamily: `Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${
              getSystem() === 'windows' ? ', twemoji mozilla' : ''
            }`,
            fontLigatures: false,
            smoothScrolling: true,
          }}
          onChange={handleYamlChange}
        />
      )}
    </BaseDialog>
  )
}
