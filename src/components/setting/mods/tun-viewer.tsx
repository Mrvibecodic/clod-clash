import { UndoRounded } from '@mui/icons-material'
import { Box, Button, TextField } from '@mui/material'
import { useLockFn } from 'ahooks'
import yaml from 'js-yaml'
import type { Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  BaseSegmented,
  BaseSplitChipEditor,
  type DialogRef,
  FormField,
  FormRow,
  FormSection,
  Switch,
} from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { useChangeCount } from '@/hooks/use-change-count'
import { useClash } from '@/hooks/use-clash'
import { useProfiles } from '@/hooks/use-profiles'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { readProfileFile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useQuery } from '@/services/query-client'
import getSystem from '@/utils/get-system'
import { areValidIpCidrs } from '@/utils/network'
import {
  TUN_RESET,
  mtuIsValid,
  splitRouteExcludeAddress,
  tunFieldsFrom,
  tunPatch,
} from '@/utils/tun-window'

import { StackModeSwitch } from './stack-mode-switch'

const OS = getSystem()

const CAPPED_STACKS = ['system', 'mixed']

const STRICT_ROUTE = [
  { value: 'auto', label: 'Auto' },
  { value: 'on', label: 'On' },
  { value: 'off', label: 'Off' },
]

const DNS_HIJACK_AUTO = [{ value: 'auto', label: 'Auto' }]

const FRESH = {
  ...tunFieldsFrom(TUN_RESET, undefined, OS),
  stack: 'auto',
  dnsHijack: 'auto',
  strictRoute: 'auto',
}

export function TunViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()

  const { runtime, ladder, patchClash } = useClash()
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { tunRuntimeStack } = useTunState()
  const { current } = useProfiles()

  const [open, setOpen] = useState(false)
  const [values, setValues] = useState(FRESH)
  // Что было при открытии: сохраняется только то, что человек поменял.
  const [initial, setInitial] = useState(FRESH)

  const effectiveStack = (
    tunRuntimeStack ??
    runtime?.tun?.stack ??
    ''
  ).toLowerCase()
  const stackChosenByHand = (values.stack || 'auto').toLowerCase() !== 'auto'

  const { data: subscriptionStack } = useQuery({
    queryKey: ['subscriptionTunStack', current?.uid, current?.updated ?? 0],
    queryFn: async () => {
      if (!current?.uid) return null
      try {
        const parsed = yaml.load(await readProfileFile(current.uid)) as {
          tun?: { stack?: unknown }
        } | null
        const stack = parsed?.tun?.stack
        return typeof stack === 'string' ? stack.trim().toLowerCase() : null
      } catch {
        return null
      }
    },
    enabled:
      OS === 'windows' &&
      open &&
      Boolean(current?.uid) &&
      !stackChosenByHand &&
      effectiveStack === 'gvisor',
    staleTime: 30000,
  })

  const stackCapped =
    OS === 'windows' &&
    !stackChosenByHand &&
    Boolean(subscriptionStack) &&
    CAPPED_STACKS.includes(subscriptionStack as string) &&
    effectiveStack === 'gvisor'

  const changes = useChangeCount(initial, values)

  const routeExcludeAddressItems = splitRouteExcludeAddress(
    values.routeExcludeAddress,
  )
  const routeExcludeAddressError =
    values.autoRoute &&
    routeExcludeAddressItems.length > 0 &&
    !areValidIpCidrs(routeExcludeAddressItems)
  const routeExcludeAddressHelperText = routeExcludeAddressError
    ? t('settings.modals.tun.messages.invalidRouteExcludeAddress')
    : t('settings.modals.tun.messages.routeExcludeAddressHint')

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)
      const opened = {
        ...tunFieldsFrom(ladder?.tun, runtime?.tun, OS),
        stack: verge?.tun_stack ?? 'auto',
        dnsHijack: verge?.tun_dns_hijack ?? 'auto',
        strictRoute: verge?.tun_strict_route ?? 'auto',
      }
      setValues(opened)
      setInitial(opened)
    },
    close: () => setOpen(false),
  }))

  const onSave = useLockFn(async () => {
    try {
      if (routeExcludeAddressError) {
        showNotice.error(
          'settings.modals.tun.messages.invalidRouteExcludeAddress',
        )
        return
      }
      if (values.mtu !== initial.mtu && !mtuIsValid(values.mtu)) {
        showNotice.error('shared.validation.numberRequired', {
          field: t('settings.modals.tun.fields.mtu'),
        })
        return
      }

      if (
        values.stack !== initial.stack ||
        values.strictRoute !== initial.strictRoute ||
        values.dnsHijack !== initial.dnsHijack
      ) {
        const overrides = {
          tun_stack: values.stack,
          tun_strict_route: values.strictRoute,
          tun_dns_hijack:
            values.dnsHijack.trim() === '' ? 'auto' : values.dnsHijack,
        }
        await patchVerge(overrides)
        mutateVerge({ ...verge, ...overrides }, false)
      }
      const tun = tunPatch(initial, values, OS)
      if (Object.keys(tun).length > 0) {
        await patchClash({ tun })
      }
      setOpen(false)
      showNotice.success('settings.modals.tun.messages.applied')
    } catch (err: any) {
      showNotice.error(err)
    }
  })

  const onReset = useLockFn(async () => {
    try {
      const overrides = {
        tun_stack: 'auto',
        tun_strict_route: 'auto',
        tun_dns_hijack: 'auto',
      }
      setValues(FRESH)
      await patchVerge(overrides)
      await patchClash({ tun: TUN_RESET })
      setInitial(FRESH)
      mutateVerge({ ...verge, ...overrides }, false)
    } catch (err: any) {
      showNotice.error(err)
    }
  })

  const help = (text: string, color = 'text.secondary') => (
    <Box sx={{ mt: 0.75, fontSize: 12.5, color }}>{text}</Box>
  )

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.tun.title')}
      titleExtra={
        <Button
          size="small"
          startIcon={<UndoRounded />}
          onClick={onReset}
          sx={{ flex: 'none', textTransform: 'none' }}
        >
          {t('shared.actions.resetToDefault')}
        </Button>
      }
      dividers
      changes={changes}
      onReset={() => setValues(initial)}
      contentSx={{ width: 512 }}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <FormSection title={t('settings.modals.tun.fields.stack')} />
      <StackModeSwitch
        value={values.stack}
        allowAuto
        onChange={(value) => {
          setValues((v) => ({
            ...v,
            stack: value,
          }))
        }}
      />
      {tunRuntimeStack &&
        help(
          t('settings.modals.tun.messages.activeStack', {
            stack: tunRuntimeStack,
          }),
        )}
      {stackCapped &&
        help(
          t('settings.modals.tun.messages.subscriptionStackCapped', {
            stack: subscriptionStack,
          }),
        )}
      {OS === 'windows' &&
        CAPPED_STACKS.includes(
          (tunRuntimeStack ?? values.stack).toLowerCase(),
        ) &&
        help(
          t('settings.modals.tun.messages.windowsStackFirewall'),
          'warning.main',
        )}

      <FormSection title={t('settings.modals.tun.sections.routing')} />
      <FormRow label={t('settings.modals.tun.fields.autoRoute')}>
        <Switch
          edge="end"
          checked={values.autoRoute}
          onChange={(_, c) =>
            setValues((v) => ({
              ...v,
              autoRoute: c,
              autoRedirect: c ? v.autoRedirect : false,
            }))
          }
        />
      </FormRow>
      {OS === 'linux' && (
        <FormRow
          label={t('settings.modals.tun.fields.autoRedirect')}
          help={t('settings.modals.tun.tooltips.autoRedirect')}
          disabled={!values.autoRoute}
        >
          <Switch
            edge="end"
            checked={values.autoRedirect}
            onChange={(_, c) =>
              setValues((v) => ({
                ...v,
                autoRedirect: v.autoRoute ? c : v.autoRedirect,
              }))
            }
            disabled={!values.autoRoute}
          />
        </FormRow>
      )}
      <FormRow label={t('settings.modals.tun.fields.strictRoute')}>
        <BaseSegmented
          value={values.strictRoute}
          options={STRICT_ROUTE}
          onChange={(mode) => setValues((v) => ({ ...v, strictRoute: mode }))}
        />
      </FormRow>
      <FormRow label={t('settings.modals.tun.fields.autoDetectInterface')}>
        <Switch
          edge="end"
          checked={values.autoDetectInterface}
          onChange={(_, c) =>
            setValues((v) => ({ ...v, autoDetectInterface: c }))
          }
        />
      </FormRow>

      <FormSection title={t('settings.modals.tun.sections.interface')} />
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'minmax(0, 1fr) minmax(0, 1fr)',
          columnGap: 1.5,
        }}
      >
        <FormField
          label={t('settings.modals.tun.fields.device')}
          sx={{ pt: 0.5 }}
        >
          <TextField
            autoComplete="new-password"
            size="small"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            fullWidth
            sx={MONO_INPUT}
            value={values.device}
            placeholder={OS === 'macos' ? 'utun' : 'Meta'}
            onChange={(e) =>
              setValues((v) => ({ ...v, device: e.target.value }))
            }
          />
        </FormField>
        <FormField label={t('settings.modals.tun.fields.mtu')} sx={{ pt: 0.5 }}>
          <TextField
            autoComplete="new-password"
            size="small"
            type="number"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            fullWidth
            sx={MONO_INPUT}
            value={values.mtu}
            placeholder="9000"
            onChange={(e) => setValues((v) => ({ ...v, mtu: e.target.value }))}
          />
        </FormField>
      </Box>
      <Box sx={{ fontSize: 12.5, color: 'text.secondary' }}>
        {t('settings.modals.tun.messages.emptyFollowsSubscription')}
      </Box>

      <FormSection title={t('settings.modals.tun.fields.dnsHijack')} />
      <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
        <BaseSegmented
          value={values.dnsHijack === 'auto' ? 'auto' : 'custom'}
          options={DNS_HIJACK_AUTO}
          onChange={() => setValues((v) => ({ ...v, dnsHijack: 'auto' }))}
          sx={{ flex: 'none' }}
        />
        <TextField
          autoComplete="new-password"
          size="small"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck="false"
          fullWidth
          sx={MONO_INPUT}
          value={values.dnsHijack === 'auto' ? '' : values.dnsHijack}
          placeholder="any:53, tcp://any:53"
          onChange={(e) =>
            setValues((v) => ({
              ...v,
              dnsHijack: e.target.value === '' ? 'auto' : e.target.value,
            }))
          }
        />
      </Box>
      {help(t('settings.modals.tun.tooltips.dnsHijack'))}

      <BaseSplitChipEditor
        value={values.routeExcludeAddress}
        placeholder="192.168.0.0/16"
        ariaLabel={t('settings.modals.tun.fields.routeExcludeAddress')}
        disabled={!values.autoRoute}
        error={routeExcludeAddressError}
        helperText={routeExcludeAddressHelperText}
        onChange={(nextValue) =>
          setValues((v) => ({ ...v, routeExcludeAddress: nextValue }))
        }
        renderHeader={(modeToggle) => (
          <FormSection
            title={t('settings.modals.tun.fields.routeExcludeAddress')}
            count={routeExcludeAddressItems.length}
            extra={modeToggle}
          />
        )}
      />
    </BaseDialog>
  )
}
