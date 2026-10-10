import { EditRounded } from '@mui/icons-material'
import {
  alpha,
  Autocomplete,
  Box,
  Button,
  InputAdornment,
  TextField,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { forwardRef, useImperativeHandle, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  BaseSplitChipEditor,
  CodeChip,
  DialogRef,
  FormField,
  FormRow,
  FormSection,
  Switch,
} from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { EditorViewer } from '@/components/profile/editor-viewer'
import { useChangeCount } from '@/hooks/use-change-count'
import { useRuntimeConfig } from '@/hooks/use-clash'
import { useSystemProxyState } from '@/hooks/use-system-proxy-state'
import { useVerge } from '@/hooks/use-verge'
import { useSystemData } from '@/providers/app-data-context'
import { getNetworkInterfacesInfo, getSystemHostname } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { debugLog } from '@/utils/debug'
import getSystem from '@/utils/get-system'
import { LOOPBACK_PROXY_HOSTS } from '@/utils/ports'

const DEFAULT_PAC = `function FindProxyForURL(url, host) {
  return "PROXY %proxy_host%:%mixed-port%; SOCKS5 %proxy_host%:%mixed-port%; DIRECT;";
}`

/** NO_PROXY validation */

// *., cdn*., *, etc.
const domain_subdomain_part = String.raw`(?:[a-z0-9\-\*]+\.|\*)*`
// .*, .cn, .moe, .co*, *
const domain_tld_part = String.raw`(?:\w{2,64}\*?|\*)`
// *epicgames*, *skk.moe, *.skk.moe, skk.*, sponsor.cdn.skk.moe, *.*, etc.
// also matches 192.168.*, 10.*, 127.0.0.*, etc. (partial ipv4)
const rDomainSimple = domain_subdomain_part + domain_tld_part

const ipv4_part = String.raw`\d{1,3}`

const ipv6_part = '(?:[a-fA-F0-9:])+'

const rLocal = `localhost|<local>|localdomain`

const BYPASS_PREVIEW = 8

const PAC_URL = `http://127.0.0.1:${import.meta.env.DEV ? 11233 : 33331}/commands/pac`

const getValidReg = (isWindows: boolean) => {
  // 127.0.0.1 (full ipv4)
  const rIPv4Unix = String.raw`(?:${ipv4_part}\.){3}${ipv4_part}(?:\/\d{1,2})?`
  const rIPv4Windows = String.raw`(?:${ipv4_part}\.){3}${ipv4_part}`

  const rIPv6Unix = String.raw`(?:${ipv6_part}:+)+${ipv6_part}(?:\/\d{1,3})?`
  const rIPv6Windows = String.raw`(?:${ipv6_part}:+)+${ipv6_part}`

  const rValidPart = `${rDomainSimple}|${
    isWindows ? rIPv4Windows : rIPv4Unix
  }|${isWindows ? rIPv6Windows : rIPv6Unix}|${rLocal}`
  const separator = isWindows ? ';' : ','
  const rValid = String.raw`^(${rValidPart})(?:${separator}\s?(${rValidPart}))*${separator}?$`

  return new RegExp(rValid)
}

const splitBypass = (value?: string) =>
  (value ?? '')
    .split(/[,\n;\r]+/)
    .map((item) => item.trim())
    .filter(Boolean)

export const SysproxyViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation()
  const systemName = getSystem()
  const isWindows = systemName === 'windows'
  const validReg = useMemo(() => getValidReg(isWindows), [isWindows])

  const [open, setOpen] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)
  const [pacEditorValue, setPacEditorValue] = useState(DEFAULT_PAC)
  const [pacEditorSavedValue, setPacEditorSavedValue] = useState(DEFAULT_PAC)
  const [saving, setSaving] = useState(false)
  const { verge, patchVerge } = useVerge()
  const [hostOptions, setHostOptions] = useState<string[]>([])
  const { data: runtime } = useRuntimeConfig()
  const lanSharing = runtime?.['allow-lan'] ?? false
  const shownHostOptions = lanSharing ? hostOptions : ['127.0.0.1']

  const { indicator: isProxyReallyEnabled, invalidateProxyState } =
    useSystemProxyState()

  const {
    enable_system_proxy: enabled,
    proxy_auto_config,
    pac_file_content,
    enable_proxy_guard,
    enable_bypass_check,
    use_default_bypass,
    system_proxy_bypass,
    proxy_guard_duration,
    proxy_host,
  } = verge ?? {}

  const snapshot = () => ({
    guard: enable_proxy_guard,
    enable_bypass_check: enable_bypass_check ?? true,
    bypass: system_proxy_bypass,
    duration: proxy_guard_duration ?? 30,
    use_default: use_default_bypass ?? true,
    pac: proxy_auto_config,
    pac_content: pac_file_content ?? DEFAULT_PAC,
    proxy_host: proxy_host ?? '127.0.0.1',
  })

  const [value, setValue] = useState(snapshot)
  const [initial, setInitial] = useState(value)
  const changes = useChangeCount(initial, value)
  const [bypassExpanded, setBypassExpanded] = useState(false)

  const separator = useMemo(() => (isWindows ? ';' : ','), [isWindows])

  const defaultBypass = () => {
    if (isWindows) {
      return 'localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>'
    }
    if (systemName === 'linux') {
      return 'localhost,127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,::1'
    }
    return '127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local,*.crashlytics.com,<local>'
  }

  const { systemProxyAddress } = useSystemData()

  const bypassError =
    value.enable_bypass_check && !value.pac && value.bypass
      ? !validReg.test(value.bypass)
      : false

  const openPacEditor = () => {
    const nextPac = value.pac_content ?? DEFAULT_PAC
    setPacEditorValue(nextPac)
    setPacEditorSavedValue(nextPac)
    setEditorOpen(true)
  }

  const handleSavePac = useLockFn(async () => {
    const nextPac =
      pacEditorValue.trim().length > 0 ? pacEditorValue : DEFAULT_PAC

    setValue((current) => ({ ...current, pac_content: nextPac }))
    setPacEditorSavedValue(nextPac)
  })

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)
      const opened = snapshot()
      setValue(opened)
      setInitial(opened)
      setBypassExpanded(false)
      fetchNetworkInterfaces()
    },
    close: () => setOpen(false),
  }))

  // Получаем сетевые интерфейсы и имя хоста
  const fetchNetworkInterfaces = async () => {
    try {
      // Получаем информацию о сетевых интерфейсах системы
      const interfaces = await getNetworkInterfacesInfo()
      const ipAddresses: string[] = []

      // Извлекаем адреса IPv4 и IPv6 из interfaces
      interfaces.forEach((iface) => {
        iface.addr.forEach((address) => {
          if (address.V4 && address.V4.ip) {
            ipAddresses.push(address.V4.ip)
          }
          if (address.V6 && address.V6.ip) {
            ipAddresses.push(address.V6.ip)
          }
        })
      })

      // Получаем имя хоста текущей системы
      let hostname = ''
      try {
        hostname = await getSystemHostname()
        debugLog('Получено имя хоста:', hostname)
      } catch (err) {
        console.error('Не удалось получить имя хоста:', err)
      }

      // Формируем список опций
      const options = [...LOOPBACK_PROXY_HOSTS]

      // Добавляем имя хоста в список, даже если оно пустая строка — фиксируем это в логе
      if (hostname) {
        // Если имя хоста не localhost и не 127.0.0.1, добавляем его
        if (hostname !== 'localhost' && hostname !== '127.0.0.1') {
          hostname = hostname + '.local'
          options.push(hostname)
          debugLog('Имя хоста добавлено в список опций:', hostname)
        } else {
          debugLog('Имя хоста дублирует существующую опцию:', hostname)
        }
      } else {
        debugLog('Имя хоста пусто')
      }

      // Добавляем IP-адреса
      options.push(...ipAddresses)

      // Убираем дубликаты
      const uniqueOptions = Array.from(new Set(options))
      debugLog('Итоговый список опций:', uniqueOptions)
      setHostOptions(uniqueOptions)
    } catch (error) {
      console.error('Не удалось получить сетевые интерфейсы:', error)
      // При ошибке предоставляем хотя бы базовые опции
      setHostOptions([...LOOPBACK_PROXY_HOSTS])
    }
  }

  const onSave = useLockFn(async () => {
    if (value.duration < 1) {
      showNotice.error('settings.modals.sysproxy.messages.durationTooShort')
      return
    }
    if (
      value.enable_bypass_check &&
      !value.pac &&
      value.bypass &&
      !validReg.test(value.bypass)
    ) {
      showNotice.error('settings.modals.sysproxy.messages.invalidBypass')
      return
    }

    // Меняем правило валидации, разрешаем IP и имя хоста
    const ipv4Regex =
      /^((25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)$/
    const ipv6Regex =
      /^(([0-9a-fA-F]{1,4}:){7,7}[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,7}:|([0-9a-fA-F]{1,4}:){1,6}:[0-9a-fA-F]{1,4}|([0-9a-fA-F]{1,4}:){1,5}(:[0-9a-fA-F]{1,4}){1,2}|([0-9a-fA-F]{1,4}:){1,4}(:[0-9a-fA-F]{1,4}){1,3}|([0-9a-fA-F]{1,4}:){1,3}(:[0-9a-fA-F]{1,4}){1,4}|([0-9a-fA-F]{1,4}:){1,2}(:[0-9a-fA-F]{1,4}){1,5}|[0-9a-fA-F]{1,4}:((:[0-9a-fA-F]{1,4}){1,6})|:((:[0-9a-fA-F]{1,4}){1,7}|:)|fe80:(:[0-9a-fA-F]{0,4}){0,4}%[0-9a-zA-Z]{1,}|::(ffff(:0{1,4}){0,1}:){0,1}((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])|([0-9a-fA-F]{1,4}:){1,4}:((25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9])\.){3,3}(25[0-5]|(2[0-4]|1{0,1}[0-9]){0,1}[0-9]))$/
    const hostnameRegex =
      /^(([a-zA-Z0-9]|[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9])\.)*([A-Za-z0-9]|[A-Za-z0-9][A-Za-z0-9-]*[A-Za-z0-9])$/

    if (
      !ipv4Regex.test(value.proxy_host) &&
      !ipv6Regex.test(value.proxy_host) &&
      !hostnameRegex.test(value.proxy_host)
    ) {
      showNotice.error('settings.modals.sysproxy.messages.invalidProxyHost')
      return
    }

    const patch: Partial<IVergeConfig> = {}

    if (value.guard !== enable_proxy_guard) {
      patch.enable_proxy_guard = value.guard
    }
    if (value.enable_bypass_check !== enable_bypass_check) {
      patch.enable_bypass_check = value.enable_bypass_check
    }
    if (value.duration !== proxy_guard_duration) {
      patch.proxy_guard_duration = value.duration
    }
    if (value.bypass !== system_proxy_bypass) {
      patch.system_proxy_bypass = value.bypass
    }
    if (value.pac !== proxy_auto_config) {
      patch.proxy_auto_config = value.pac
    }
    if (value.use_default !== use_default_bypass) {
      patch.use_default_bypass = value.use_default
    }

    if (value.pac_content !== pac_file_content) {
      patch.pac_file_content = value.pac_content
    }

    // Обрабатываем адрес IPv6: если это IPv6 без квадратных скобок, добавляем их
    let proxyHost = value.proxy_host
    if (
      ipv6Regex.test(proxyHost) &&
      !proxyHost.startsWith('[') &&
      !proxyHost.endsWith(']')
    ) {
      proxyHost = `[${proxyHost}]`
    }

    if (proxyHost !== proxy_host) {
      patch.proxy_host = proxyHost
    }

    // Окно ждёт записи: закрывается только когда настройки приняты, а при
    // отказе остаётся открытым со всем введённым — ничего не набирать заново.
    setSaving(true)
    try {
      if (Object.keys(patch).length > 0) {
        await patchVerge(patch)
      }
    } catch (err) {
      console.error('Не удалось сохранить конфигурацию:', err)
      showNotice.error(err)
      return
    } finally {
      setSaving(false)
    }
    setOpen(false)
    void invalidateProxyState().catch((err) =>
      console.error('Не удалось обновить состояние прокси:', err),
    )
  })

  const standardBypass = splitBypass(defaultBypass())
  const shownBypass = bypassExpanded
    ? standardBypass
    : standardBypass.slice(0, BYPASS_PREVIEW)

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.sysproxy.title')}
      dividers
      changes={changes}
      onReset={() => setValue(initial)}
      contentSx={{ width: 532 }}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => !saving && setOpen(false)}
      onCancel={() => !saving && setOpen(false)}
      onOk={onSave}
      loading={saving}
      disableCancel={saving}
    >
      <Box
        sx={({ palette }) => ({
          display: 'flex',
          flexWrap: 'wrap',
          alignItems: 'center',
          columnGap: 1.25,
          rowGap: 0.75,
          px: 1.5,
          py: 1,
          mb: 0.5,
          borderRadius: '10px',
          bgcolor: alpha(palette.text.primary, 0.045),
        })}
      >
        <Box component="span" sx={{ fontSize: 13, color: 'text.secondary' }}>
          {t('settings.modals.sysproxy.fieldsets.currentStatus')}
        </Box>
        <Box
          component="span"
          title={t('settings.modals.sysproxy.fields.enableStatus')}
          sx={({ palette }) => {
            const accent = isProxyReallyEnabled
              ? palette.success.main
              : palette.text.secondary
            return {
              display: 'inline-flex',
              alignItems: 'center',
              gap: 0.75,
              px: 1,
              py: 0.25,
              borderRadius: '999px',
              fontSize: 12,
              fontWeight: 600,
              color: accent,
              bgcolor: alpha(accent, 0.14),
              transition: 'background-color 150ms, color 150ms',
              '&::before': {
                content: '""',
                width: 6,
                height: 6,
                borderRadius: '50%',
                bgcolor: 'currentColor',
              },
            }
          }}
        >
          {isProxyReallyEnabled
            ? t('shared.statuses.enabled')
            : t('shared.statuses.disabled')}
        </Box>
        <CodeChip
          title={
            value.pac
              ? t('settings.modals.sysproxy.fields.pacUrl')
              : t('settings.modals.sysproxy.fields.serverAddr')
          }
          sx={{ ml: 'auto', maxWidth: '100%', fontSize: 12.5 }}
        >
          {value.pac ? PAC_URL : systemProxyAddress}
        </CodeChip>
      </Box>

      <FormSection title={t('settings.modals.sysproxy.sections.address')} />
      <FormField
        label={t('settings.modals.sysproxy.fields.proxyHost')}
        help={t('settings.modals.sysproxy.tooltips.proxyHost')}
        sx={{ pt: 0.5 }}
      >
        <Autocomplete
          size="small"
          fullWidth
          options={shownHostOptions}
          value={value.proxy_host}
          freeSolo
          renderInput={(params) => (
            <TextField
              {...params}
              placeholder="127.0.0.1"
              size="small"
              sx={MONO_INPUT}
            />
          )}
          onChange={(_, newValue) => {
            setValue((v) => ({
              ...v,
              proxy_host: newValue || '127.0.0.1',
            }))
          }}
          onInputChange={(_, newInputValue) => {
            setValue((v) => ({
              ...v,
              proxy_host: newInputValue || '127.0.0.1',
            }))
          }}
        />
      </FormField>
      <FormRow label={t('settings.modals.sysproxy.fields.usePacMode')}>
        <Switch
          edge="end"
          disabled={!enabled}
          checked={value.pac}
          onChange={(_, e) => setValue((v) => ({ ...v, pac: e }))}
        />
      </FormRow>
      {value.pac && (
        <FormRow label={t('settings.modals.sysproxy.fields.pacScriptContent')}>
          <Button
            startIcon={<EditRounded />}
            variant="outlined"
            size="small"
            onClick={openPacEditor}
          >
            {t('settings.modals.sysproxy.actions.editPac')}
          </Button>
          {editorOpen && (
            <EditorViewer
              open={true}
              title={t('settings.modals.sysproxy.actions.editPac')}
              value={pacEditorValue}
              language="javascript"
              path="sysproxy-pac.js"
              dirty={pacEditorValue !== pacEditorSavedValue}
              onChange={setPacEditorValue}
              onSave={handleSavePac}
              onClose={() => setEditorOpen(false)}
            />
          )}
        </FormRow>
      )}

      <FormSection title={t('settings.modals.sysproxy.sections.guard')} />
      <FormRow
        label={t('settings.modals.sysproxy.fields.proxyGuard')}
        help={t('settings.modals.sysproxy.tooltips.proxyGuard')}
      >
        <Switch
          edge="end"
          disabled={!enabled}
          checked={value.guard}
          onChange={(_, e) => setValue((v) => ({ ...v, guard: e }))}
        />
      </FormRow>
      <FormRow
        label={t('settings.modals.sysproxy.fields.guardDuration')}
        disabled={!value.guard}
      >
        <TextField
          disabled={!enabled}
          size="small"
          value={value.duration}
          sx={{
            width: 110,
            opacity: value.guard ? 1 : 0.6,
            transition: 'opacity 150ms',
            ...MONO_INPUT,
          }}
          slotProps={{
            input: {
              endAdornment: <InputAdornment position="end">s</InputAdornment>,
            },
          }}
          onChange={(e) => {
            setValue((v) => ({
              ...v,
              duration: +e.target.value.replace(/\D/g, ''),
            }))
          }}
        />
      </FormRow>

      {!value.pac && (
        <>
          <FormSection title={t('settings.modals.sysproxy.sections.bypass')} />
          <FormRow
            label={t('settings.modals.sysproxy.fields.alwaysUseDefaultBypass')}
          >
            <Switch
              edge="end"
              disabled={!enabled}
              checked={value.use_default}
              onChange={(_, e) => setValue((v) => ({ ...v, use_default: e }))}
            />
          </FormRow>
          <FormRow
            label={t('settings.modals.sysproxy.fields.enableBypassCheck')}
          >
            <Switch
              edge="end"
              disabled={!enabled}
              checked={value.enable_bypass_check}
              onChange={(_, e) =>
                setValue((v) => ({ ...v, enable_bypass_check: e }))
              }
            />
          </FormRow>
          <BaseSplitChipEditor
            value={value.bypass ?? ''}
            separator={separator}
            disabled={!enabled}
            error={bypassError}
            helperText={
              bypassError
                ? t('settings.modals.sysproxy.messages.invalidBypass')
                : undefined
            }
            placeholder="localhost"
            ariaLabel={t('settings.modals.sysproxy.fields.proxyBypass')}
            onChange={(nextValue) => {
              setValue((v) => ({ ...v, bypass: nextValue }))
            }}
            renderHeader={(modeToggle) => (
              <FormSection
                title={t('settings.modals.sysproxy.fields.proxyBypass')}
                count={splitBypass(value.bypass).length}
                extra={modeToggle}
              />
            )}
          />
        </>
      )}

      {!value.pac && (value.use_default || !value.bypass) && (
        <>
          <FormSection
            title={t('settings.modals.sysproxy.fields.bypass')}
            count={standardBypass.length}
          />
          <Box
            sx={{
              display: 'flex',
              flexWrap: 'wrap',
              alignItems: 'center',
              gap: 0.75,
              pb: 0.5,
            }}
          >
            {shownBypass.map((item) => (
              <CodeChip key={item}>{item}</CodeChip>
            ))}
            {standardBypass.length > BYPASS_PREVIEW && (
              <Button
                size="small"
                onClick={() => setBypassExpanded((x) => !x)}
                sx={{ minWidth: 0, py: 0, textTransform: 'none' }}
              >
                {bypassExpanded
                  ? t('settings.modals.sysproxy.actions.showLess')
                  : t('settings.modals.sysproxy.actions.showMore', {
                      count: standardBypass.length - BYPASS_PREVIEW,
                    })}
              </Button>
            )}
          </Box>
        </>
      )}
    </BaseDialog>
  )
})
