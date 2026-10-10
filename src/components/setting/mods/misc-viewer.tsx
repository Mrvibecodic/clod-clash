import { InputAdornment, MenuItem, Select, TextField } from '@mui/material'
import { useLockFn } from 'ahooks'
import { forwardRef, useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  DialogRef,
  FormField,
  FormRow,
  FormSection,
  Switch,
} from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { useChangeCount } from '@/hooks/use-change-count'
import { useVerge } from '@/hooks/use-verge'
import {
  LATENCY_TIMEOUT_MAX,
  LATENCY_TIMEOUT_MIN,
  effectiveLatencyTimeout,
} from '@/services/delay'
import { showNotice } from '@/services/notice-service'
import { isValidUrl } from '@/utils/network'

// Те же значения, что у шаблона настроек и запасных значений бэкенда
// (`IVerge::DEFAULT_APP_LOG_MAX_SIZE` / `DEFAULT_APP_LOG_MAX_COUNT`): у формы
// было собственное умолчание, и она показывала не то, с чем работает ротация.
const DEFAULT_APP_LOG_MAX_SIZE = 1024
const DEFAULT_APP_LOG_MAX_COUNT = 8

const NUMBER_PROPS = {
  autoComplete: 'new-password',
  size: 'small',
  type: 'number',
  autoCorrect: 'off',
  autoCapitalize: 'off',
  spellCheck: 'false',
} as const

const withUnit = (unit: string) => ({
  input: {
    endAdornment: (
      <InputAdornment position="end" sx={{ '& p': { fontSize: 12.5 } }}>
        {unit}
      </InputAdornment>
    ),
  },
})

export const MiscViewer = forwardRef<DialogRef>((props, ref) => {
  const { t } = useTranslation()
  const { verge, patchVerge } = useVerge()

  const [open, setOpen] = useState(false)
  const [values, setValues] = useState(() => ({
    appLogLevel: 'info',
    appLogMaxSize: DEFAULT_APP_LOG_MAX_SIZE,
    appLogMaxCount: DEFAULT_APP_LOG_MAX_COUNT,
    verboseDiagnostics: false,
    autoCloseConnection: true,
    autoCloseConnectionHome: false,
    autoCheckUpdate: true,
    enableBuiltinEnhanced: true,
    proxyLayoutColumn: 6,
    defaultLatencyTest: '',
    autoLogClean: 2,
    defaultLatencyTimeout: 10000,
  }))
  const [initialValues, setInitialValues] = useState(values)
  const changes = useChangeCount(initialValues, values)

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)
      const next = {
        appLogLevel: verge?.app_log_level ?? 'info',
        appLogMaxSize: verge?.app_log_max_size ?? DEFAULT_APP_LOG_MAX_SIZE,
        appLogMaxCount: verge?.app_log_max_count ?? DEFAULT_APP_LOG_MAX_COUNT,
        verboseDiagnostics: verge?.enable_verbose_diagnostics ?? false,
        autoCloseConnection: verge?.auto_close_connection ?? true,
        autoCloseConnectionHome: verge?.auto_close_connection_home ?? false,
        autoCheckUpdate: verge?.auto_check_update ?? true,
        enableBuiltinEnhanced: verge?.enable_builtin_enhanced ?? true,
        proxyLayoutColumn: verge?.proxy_layout_column || 6,
        defaultLatencyTest: verge?.default_latency_test || '',
        autoLogClean: verge?.auto_log_clean || 0,
        defaultLatencyTimeout: effectiveLatencyTimeout(
          verge?.default_latency_timeout,
        ),
      }
      setValues(next)
      setInitialValues(next)
    },
    close: () => setOpen(false),
  }))

  // clod:УП-33 — негодный адрес проверки в ядро и так не уходит, но человек
  // узнавал об этом только по кнопке проверки; поле говорит об этом само.
  const badTestUrl =
    values.defaultLatencyTest.trim() !== '' &&
    !isValidUrl(values.defaultLatencyTest)

  const onSave = useLockFn(async () => {
    if (badTestUrl) {
      showNotice.error('proxies.page.messages.badTestUrl')
      return
    }
    // Ядро разбирает тайм-аут как int16; отрицательный делал все узлы «тайм-аутом»
    if (
      effectiveLatencyTimeout(values.defaultLatencyTimeout) !==
      values.defaultLatencyTimeout
    ) {
      showNotice.error('shared.validation.numberRange', {
        field: t('settings.modals.misc.fields.defaultLatencyTimeout'),
        min: LATENCY_TIMEOUT_MIN,
        max: LATENCY_TIMEOUT_MAX,
      })
      return
    }
    try {
      await patchVerge({
        app_log_level: values.appLogLevel,
        app_log_max_size: values.appLogMaxSize,
        app_log_max_count: values.appLogMaxCount,
        enable_verbose_diagnostics: values.verboseDiagnostics,
        auto_close_connection: values.autoCloseConnection,
        auto_close_connection_home: values.autoCloseConnectionHome,
        auto_check_update: values.autoCheckUpdate,
        enable_builtin_enhanced: values.enableBuiltinEnhanced,
        proxy_layout_column: values.proxyLayoutColumn,
        default_latency_test: values.defaultLatencyTest,
        default_latency_timeout: values.defaultLatencyTimeout,
        auto_log_clean: values.autoLogClean as any,
      })
      setOpen(false)
    } catch (err) {
      showNotice.error(err)
    }
  })

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.misc.title')}
      dividers
      changes={changes}
      onReset={() => setValues(initialValues)}
      contentSx={{ width: 552 }}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <FormSection title={t('settings.modals.misc.sections.appLog')} />
      <FormRow label={t('settings.modals.misc.fields.appLogLevel')}>
        <Select
          size="small"
          sx={{ width: 140, fontSize: 14 }}
          value={values.appLogLevel}
          onChange={(e) =>
            setValues((v) => ({
              ...v,
              appLogLevel: e.target.value as string,
            }))
          }
        >
          {['trace', 'debug', 'info', 'warn', 'error', 'silent'].map((i) => (
            <MenuItem value={i} key={i}>
              {i[0].toUpperCase() + i.slice(1).toLowerCase()}
            </MenuItem>
          ))}
        </Select>
      </FormRow>

      <FormRow label={t('settings.modals.misc.fields.appLogMaxSize')}>
        <TextField
          {...NUMBER_PROPS}
          sx={{ width: 140, ...MONO_INPUT }}
          value={values.appLogMaxSize}
          onChange={(e) =>
            setValues((v) => ({
              ...v,
              appLogMaxSize: Math.max(
                1,
                parseInt(e.target.value) || DEFAULT_APP_LOG_MAX_SIZE,
              ),
            }))
          }
          slotProps={withUnit(t('shared.units.kilobytes'))}
        />
      </FormRow>

      <FormRow label={t('settings.modals.misc.fields.appLogMaxCount')}>
        <TextField
          {...NUMBER_PROPS}
          sx={{ width: 140, ...MONO_INPUT }}
          value={values.appLogMaxCount}
          onChange={(e) =>
            setValues((v) => ({
              ...v,
              appLogMaxCount: Math.max(1, parseInt(e.target.value) || 1),
            }))
          }
          slotProps={withUnit(t('shared.units.files'))}
        />
      </FormRow>

      <FormRow label={t('settings.modals.misc.fields.autoLogClean')}>
        <Select
          size="small"
          sx={{ width: 190, fontSize: 14 }}
          value={values.autoLogClean}
          onChange={(e) =>
            setValues((v) => ({
              ...v,
              autoLogClean: e.target.value as number,
            }))
          }
        >
          {/* 1: 1 день, 2: 7 дней, 3: 30 дней, 4: 90 дней*/}
          {[
            {
              key: t('settings.modals.misc.options.autoLogClean.never'),
              value: 0,
            },
            {
              key: t('settings.modals.misc.options.autoLogClean.retainDays', {
                n: 1,
              }),
              value: 1,
            },
            {
              key: t('settings.modals.misc.options.autoLogClean.retainDays', {
                n: 7,
              }),
              value: 2,
            },
            {
              key: t('settings.modals.misc.options.autoLogClean.retainDays', {
                n: 30,
              }),
              value: 3,
            },
            {
              key: t('settings.modals.misc.options.autoLogClean.retainDays', {
                n: 90,
              }),
              value: 4,
            },
          ].map((i) => (
            <MenuItem key={i.value} value={i.value}>
              {i.key}
            </MenuItem>
          ))}
        </Select>
      </FormRow>

      <FormRow
        label={t('settings.modals.misc.fields.verboseDiagnostics')}
        help={t('settings.modals.misc.tooltips.verboseDiagnostics')}
      >
        <Switch
          edge="end"
          checked={values.verboseDiagnostics}
          onChange={(_, c) =>
            setValues((v) => ({ ...v, verboseDiagnostics: c }))
          }
        />
      </FormRow>

      <FormSection title={t('settings.modals.misc.sections.connections')} />
      <FormRow
        label={t('settings.modals.misc.fields.autoCloseConnections')}
        help={t('settings.modals.misc.tooltips.autoCloseConnections')}
      >
        <Switch
          edge="end"
          checked={values.autoCloseConnection}
          onChange={(_, c) =>
            setValues((v) => ({ ...v, autoCloseConnection: c }))
          }
        />
      </FormRow>

      <FormRow
        label={t('settings.modals.misc.fields.autoCloseConnectionsHome')}
        help={t('settings.modals.misc.tooltips.autoCloseConnectionsHome')}
        disabled={!values.autoCloseConnection}
      >
        <Switch
          edge="end"
          checked={values.autoCloseConnectionHome}
          disabled={!values.autoCloseConnection}
          onChange={(_, c) =>
            setValues((v) => ({ ...v, autoCloseConnectionHome: c }))
          }
        />
      </FormRow>

      <FormSection title={t('settings.modals.misc.sections.updates')} />
      <FormRow label={t('settings.modals.misc.fields.autoCheckUpdate')}>
        <Switch
          edge="end"
          checked={values.autoCheckUpdate}
          onChange={(_, c) => setValues((v) => ({ ...v, autoCheckUpdate: c }))}
        />
      </FormRow>

      <FormRow
        label={t('settings.modals.misc.fields.enableBuiltinEnhanced')}
        help={t('settings.modals.misc.tooltips.enableBuiltinEnhanced')}
      >
        <Switch
          edge="end"
          checked={values.enableBuiltinEnhanced}
          onChange={(_, c) =>
            setValues((v) => ({ ...v, enableBuiltinEnhanced: c }))
          }
        />
      </FormRow>

      <FormSection title={t('settings.modals.misc.sections.proxies')} />
      <FormRow label={t('settings.modals.misc.fields.proxyLayoutColumns')}>
        <Select
          size="small"
          sx={{ width: 160, fontSize: 14 }}
          value={values.proxyLayoutColumn}
          onChange={(e) =>
            setValues((v) => ({
              ...v,
              proxyLayoutColumn: e.target.value as number,
            }))
          }
        >
          <MenuItem value={6} key={6}>
            {t('settings.modals.misc.options.proxyLayoutColumns.auto')}
          </MenuItem>
          {[1, 2, 3, 4, 5].map((i) => (
            <MenuItem value={i} key={i}>
              {i}
            </MenuItem>
          ))}
        </Select>
      </FormRow>

      <FormField
        label={t('settings.modals.misc.fields.defaultLatencyTest')}
        help={t('settings.modals.misc.tooltips.defaultLatencyTest')}
        sx={{ borderTop: 1, borderBottom: 1, borderColor: 'divider' }}
      >
        <TextField
          autoComplete="new-password"
          size="small"
          fullWidth
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck="false"
          value={values.defaultLatencyTest}
          error={badTestUrl}
          helperText={
            badTestUrl ? t('proxies.page.messages.badTestUrl') : undefined
          }
          placeholder="http://cp.cloudflare.com/generate_204"
          sx={MONO_INPUT}
          onChange={(e) =>
            setValues((v) => ({ ...v, defaultLatencyTest: e.target.value }))
          }
        />
      </FormField>

      <FormRow label={t('settings.modals.misc.fields.defaultLatencyTimeout')}>
        <TextField
          {...NUMBER_PROPS}
          sx={{ width: 140, ...MONO_INPUT }}
          value={values.defaultLatencyTimeout}
          placeholder="10000"
          onChange={(e) =>
            setValues((v) => ({
              ...v,
              defaultLatencyTimeout: parseInt(e.target.value),
            }))
          }
          slotProps={withUnit(t('shared.units.milliseconds'))}
        />
      </FormRow>
    </BaseDialog>
  )
})
