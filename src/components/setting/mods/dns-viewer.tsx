import { RestartAltRounded } from '@mui/icons-material'
import {
  Box,
  Button,
  FormControl,
  List,
  ListItem,
  ListItemText,
  MenuItem,
  Select,
  styled,
  TextField,
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

const Item = styled(ListItem)(() => ({
  padding: '5px 2px',
  '& textarea': {
    lineHeight: 1.5,
    fontSize: 14,
    resize: 'vertical',
  },
}))

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
  return str
    .split(',')
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
  const [values, setValues] = useState<{
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
  }>({
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

  return (
    <BaseDialog
      open={open}
      disableEnforceFocus={!visualization}
      title={
        <Box
          sx={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
          }}
        >
          {t('settings.modals.dns.dialog.title')}
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <Button
              variant="outlined"
              size="small"
              color="warning"
              startIcon={<RestartAltRounded />}
              disabled={seeding}
              onClick={showTheSubscription}
            >
              {t('settings.modals.dns.actions.asInSubscription')}
            </Button>
            <Button
              variant="contained"
              size="small"
              onClick={() => {
                if (visualization || updateValuesFromYaml()) {
                  setVisualization(!visualization)
                }
              }}
            >
              {visualization
                ? t('shared.editorModes.advanced')
                : t('shared.editorModes.visualization')}
            </Button>
          </Box>
        </Box>
      }
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
      <Typography
        variant="body2"
        color="warning.main"
        sx={{ mb: 2, mt: 0, fontStyle: 'italic' }}
      >
        {t('settings.modals.dns.dialog.warning')}
      </Typography>

      <Typography variant="body2" color="text.secondary" sx={{ mb: 2, mt: -1 }}>
        {t('settings.modals.dns.dialog.replacesSubscription')}
      </Typography>

      {visualization ? (
        <List>
          <Typography
            variant="subtitle1"
            sx={{ mt: 1, mb: 1, fontWeight: 'bold' }}
          >
            {t('settings.modals.dns.sections.general')}
          </Typography>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.enable')}
              secondary={t('settings.modals.dns.fields.enableHint')}
            />
            <Switch
              edge="end"
              checked={values.enable}
              onChange={handleChange('enable')}
            />
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.listen')}
              secondary={t('settings.modals.dns.fields.listenHint')}
            />
            <TextField
              size="small"
              autoComplete="off"
              spellCheck="false"
              value={values.listen}
              onChange={handleChange('listen')}
              placeholder="127.0.0.1:1053"
              sx={{ width: 150 }}
            />
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.enhancedMode')}
              secondary={t('settings.modals.dns.fields.enhancedModeHint')}
            />
            <FormControl size="small" sx={{ width: 150 }}>
              <Select
                value={values.enhancedMode}
                onChange={handleChange('enhancedMode')}
              >
                <MenuItem value="fake-ip">fake-ip</MenuItem>
                <MenuItem value="redir-host">redir-host</MenuItem>
              </Select>
            </FormControl>
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.fakeIpRange')}
            />
            <TextField
              size="small"
              autoComplete="off"
              spellCheck="false"
              value={values.fakeIpRange}
              onChange={handleChange('fakeIpRange')}
              placeholder="198.18.0.1/16"
              sx={{ width: 150 }}
            />
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.fakeIpRange6')}
            />
            <TextField
              size="small"
              autoComplete="off"
              spellCheck="false"
              value={values.fakeIpRange6}
              onChange={handleChange('fakeIpRange6')}
              placeholder="2001:2::0/64"
              sx={{ width: 200 }}
            />
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.fakeIpFilterMode')}
            />
            <FormControl size="small" sx={{ width: 150 }}>
              <Select
                value={values.fakeIpFilterMode}
                onChange={handleChange('fakeIpFilterMode')}
              >
                <MenuItem value="blacklist">blacklist</MenuItem>
                <MenuItem value="whitelist">whitelist</MenuItem>
              </Select>
            </FormControl>
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.ipv6.label')}
              secondary={t('settings.modals.dns.fields.ipv6.description')}
            />
            <Switch
              edge="end"
              checked={values.ipv6}
              onChange={handleChange('ipv6')}
            />
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.preferH3.label')}
              secondary={t('settings.modals.dns.fields.preferH3.description')}
            />
            <Switch
              edge="end"
              checked={values.preferH3}
              onChange={handleChange('preferH3')}
            />
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.respectRules.label')}
              secondary={t(
                'settings.modals.dns.fields.respectRules.description',
              )}
            />
            <Switch
              edge="end"
              checked={values.respectRules}
              onChange={handleChange('respectRules')}
            />
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.useHosts.label')}
              secondary={t('settings.modals.dns.fields.useHosts.description')}
            />
            <Select
              size="small"
              sx={{ width: 160 }}
              value={values.useHosts}
              onChange={handleChange('useHosts')}
            >
              <MenuItem value="auto">
                {t('settings.modals.dns.options.hosts.auto')}
              </MenuItem>
              <MenuItem value="on">
                {t('settings.modals.dns.options.hosts.on')}
              </MenuItem>
              <MenuItem value="off">
                {t('settings.modals.dns.options.hosts.off')}
              </MenuItem>
            </Select>
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.useSystemHosts.label')}
              secondary={t(
                'settings.modals.dns.fields.useSystemHosts.description',
              )}
            />
            <Select
              size="small"
              sx={{ width: 160 }}
              value={values.useSystemHosts}
              onChange={handleChange('useSystemHosts')}
            >
              <MenuItem value="auto">
                {t('settings.modals.dns.options.hosts.auto')}
              </MenuItem>
              <MenuItem value="on">
                {t('settings.modals.dns.options.hosts.on')}
              </MenuItem>
              <MenuItem value="off">
                {t('settings.modals.dns.options.hosts.off')}
              </MenuItem>
            </Select>
          </Item>

          <Item>
            <ListItemText
              primary={t('settings.modals.dns.fields.directPolicy.label')}
              secondary={t(
                'settings.modals.dns.fields.directPolicy.description',
              )}
            />
            <Switch
              edge="end"
              checked={values.directNameserverFollowPolicy}
              onChange={handleChange('directNameserverFollowPolicy')}
            />
          </Item>

          <Item sx={{ flexDirection: 'column', alignItems: 'flex-start' }}>
            <ListItemText
              primary={t('settings.modals.dns.fields.defaultNameserver.label')}
              secondary={t(
                'settings.modals.dns.fields.defaultNameserver.description',
              )}
            />
            <TextField
              fullWidth
              multiline
              minRows={2}
              maxRows={3}
              size="small"
              spellCheck="false"
              value={values.defaultNameserver}
              onChange={handleChange('defaultNameserver')}
              placeholder="system,223.6.6.6, 8.8.8.8, 2400:3200::1, 2001:4860:4860::8888"
            />
          </Item>

          <Item sx={{ flexDirection: 'column', alignItems: 'flex-start' }}>
            <ListItemText
              primary={t('settings.modals.dns.fields.nameserver.label')}
              secondary={t('settings.modals.dns.fields.nameserver.description')}
            />
            <TextField
              fullWidth
              multiline
              minRows={2}
              maxRows={4}
              size="small"
              spellCheck="false"
              value={values.nameserver}
              onChange={handleChange('nameserver')}
              placeholder="8.8.8.8, https://doh.pub/dns-query, https://dns.alidns.com/dns-query"
            />
          </Item>

          <Item sx={{ flexDirection: 'column', alignItems: 'flex-start' }}>
            <ListItemText
              primary={t('settings.modals.dns.fields.proxy.label')}
              secondary={t('settings.modals.dns.fields.proxy.description')}
            />
            <TextField
              fullWidth
              multiline
              minRows={2}
              maxRows={3}
              size="small"
              spellCheck="false"
              value={values.proxyServerNameserver}
              onChange={handleChange('proxyServerNameserver')}
              placeholder="https://doh.pub/dns-query, https://dns.alidns.com/dns-query"
            />
          </Item>

          <Item sx={{ flexDirection: 'column', alignItems: 'flex-start' }}>
            <ListItemText
              primary={t('settings.modals.dns.fields.directNameserver.label')}
              secondary={t(
                'settings.modals.dns.fields.directNameserver.description',
              )}
            />
            <TextField
              fullWidth
              multiline
              minRows={2}
              maxRows={3}
              size="small"
              spellCheck="false"
              value={values.directNameserver}
              onChange={handleChange('directNameserver')}
              placeholder="system, 223.6.6.6"
            />
          </Item>

          <Item sx={{ flexDirection: 'column', alignItems: 'flex-start' }}>
            <ListItemText
              primary={t('settings.modals.dns.fields.fakeIpFilter.label')}
              secondary={t(
                'settings.modals.dns.fields.fakeIpFilter.description',
              )}
            />
            <TextField
              fullWidth
              multiline
              minRows={2}
              maxRows={4}
              size="small"
              spellCheck="false"
              value={values.fakeIpFilter}
              onChange={handleChange('fakeIpFilter')}
              placeholder="*.lan, *.local, localhost.ptlogin2.qq.com"
            />
          </Item>

          <Item sx={{ flexDirection: 'column', alignItems: 'flex-start' }}>
            <ListItemText
              primary={t('settings.modals.dns.fields.nameserverPolicy.label')}
              secondary={t(
                'settings.modals.dns.fields.nameserverPolicy.description',
              )}
            />
            <TextField
              fullWidth
              multiline
              minRows={2}
              maxRows={4}
              size="small"
              spellCheck="false"
              value={values.nameserverPolicy}
              onChange={handleChange('nameserverPolicy')}
              placeholder="+.arpa=10.0.0.1, rule-set:cn=https://doh.pub/dns-query;https://dns.alidns.com/dns-query"
            />
          </Item>

          <Typography
            variant="subtitle1"
            sx={{ mt: 3, mb: 0, fontWeight: 'bold' }}
          >
            {t('settings.modals.dns.sections.hosts')}
          </Typography>

          <Item sx={{ flexDirection: 'column', alignItems: 'flex-start' }}>
            <ListItemText
              primary={t('settings.modals.dns.fields.hosts.label')}
              secondary={t('settings.modals.dns.fields.hosts.description')}
            />
            <TextField
              fullWidth
              multiline
              minRows={2}
              maxRows={4}
              size="small"
              spellCheck="false"
              value={values.hosts}
              onChange={handleChange('hosts')}
              placeholder="*.clash.dev=127.0.0.1, alpha.clash.dev=::1, test.com=1.1.1.1;2.2.2.2, baidu.com=google.com"
            />
          </Item>
        </List>
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
              top: 33,
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
