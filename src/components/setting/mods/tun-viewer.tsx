import {
  Box,
  Button,
  ButtonGroup,
  List,
  ListItem,
  ListItemText,
  TextField,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import type { Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  BaseSplitChipEditor,
  TooltipIcon,
  DialogRef,
  Switch,
} from '@/components/base'
import { useClash } from '@/hooks/use-clash'
import { useTunState } from '@/hooks/use-tun-state'
import { useVerge } from '@/hooks/use-verge'
import { enhanceProfiles } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import getSystem from '@/utils/get-system'
import { areValidIpCidrs } from '@/utils/network'

import { StackModeSwitch } from './stack-mode-switch'

const OS = getSystem()

const splitRouteExcludeAddress = (value: string) =>
  value
    .split(/[,\n;\r]+/)
    .map((item) => item.trim())
    .filter(Boolean)

export function TunViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()

  const { clash, mutateClash, patchClash } = useClash()
  const { verge, mutateVerge, patchVerge } = useVerge()
  const { tunRuntimeStack } = useTunState()

  const [open, setOpen] = useState(false)
  const [values, setValues] = useState({
    stack: 'auto',
    device: OS === 'macos' ? 'utun1024' : 'Mihomo',
    autoRoute: true,
    routeExcludeAddress: '',
    autoRedirect: false,
    autoDetectInterface: true,
    dnsHijack: 'auto',
    strictRoute: 'auto',
    mtu: 1500,
  })

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
      const nextAutoRoute = clash?.tun['auto-route'] ?? true
      const rawAutoRedirect = clash?.tun['auto-redirect'] ?? false
      const computedAutoRedirect =
        OS === 'linux' ? (nextAutoRoute ? rawAutoRedirect : false) : false
      setValues({
        stack: verge?.tun_stack ?? 'auto',
        device: clash?.tun.device ?? (OS === 'macos' ? 'utun1024' : 'Mihomo'),
        autoRoute: nextAutoRoute,
        routeExcludeAddress: (clash?.tun['route-exclude-address'] ?? []).join(
          ',',
        ),
        autoRedirect: computedAutoRedirect,
        autoDetectInterface: clash?.tun['auto-detect-interface'] ?? true,
        dnsHijack: verge?.tun_dns_hijack ?? 'auto',
        strictRoute: verge?.tun_strict_route ?? 'auto',
        mtu: clash?.tun.mtu ?? 1500,
      })
    },
    close: () => setOpen(false),
  }))

  const onSave = useLockFn(async () => {
    try {
      const routeExcludeAddress = routeExcludeAddressItems

      if (routeExcludeAddressError) {
        showNotice.error(
          'settings.modals.tun.messages.invalidRouteExcludeAddress',
        )
        return
      }

      const tun: IConfigData['tun'] = {
        ...clash?.tun,
        device:
          values.device === ''
            ? OS === 'macos'
              ? 'utun1024'
              : 'Mihomo'
            : values.device,
        'auto-route': values.autoRoute,
        'route-exclude-address': routeExcludeAddress,
        ...(OS === 'linux'
          ? {
              'auto-redirect': values.autoRedirect,
            }
          : {}),
        'auto-detect-interface': values.autoDetectInterface,
        mtu: values.mtu ?? 1500,
      }
      const overrides = {
        tun_stack: values.stack,
        tun_strict_route: values.strictRoute,
        tun_dns_hijack:
          values.dnsHijack.trim() === '' ? 'auto' : values.dnsHijack,
      }
      await patchClash({ tun })
      await patchVerge(overrides)
      await mutateClash(
        (old) => ({
          ...old!,
          tun: { ...old!.tun, ...tun },
        }),
        false,
      )
      mutateVerge({ ...verge, ...overrides }, false)
      setOpen(false)
      showNotice.success('settings.modals.tun.messages.applied')
      void enhanceProfiles().catch((err: any) => {
        showNotice.error(err)
      })
    } catch (err: any) {
      showNotice.error(err)
    }
  })

  return (
    <BaseDialog
      open={open}
      title={
        <Box sx={{ display: 'flex', justifyContent: 'space-between', gap: 1 }}>
          <Typography variant="h6">{t('settings.modals.tun.title')}</Typography>
          <Button
            variant="outlined"
            size="small"
            onClick={async () => {
              const tun: IConfigData['tun'] = {
                ...clash?.tun,
                device: OS === 'macos' ? 'utun1024' : 'Mihomo',
                'auto-route': true,
                ...(OS === 'linux'
                  ? {
                      'auto-redirect': false,
                    }
                  : {}),
                'auto-detect-interface': true,
                'route-exclude-address': [],
                mtu: 1500,
              }
              const overrides = {
                tun_stack: 'auto',
                tun_strict_route: 'auto',
                tun_dns_hijack: 'auto',
              }
              setValues({
                stack: 'auto',
                device: OS === 'macos' ? 'utun1024' : 'Mihomo',
                autoRoute: true,
                routeExcludeAddress: '',
                autoRedirect: false,
                autoDetectInterface: true,
                dnsHijack: 'auto',
                strictRoute: 'auto',
                mtu: 1500,
              })
              await patchClash({ tun })
              await patchVerge(overrides)
              await mutateClash(
                (old) => ({
                  ...old!,
                  tun: { ...old!.tun, ...tun },
                }),
                false,
              )
              mutateVerge({ ...verge, ...overrides }, false)
              void enhanceProfiles().catch((err: any) => {
                showNotice.error(err)
              })
            }}
          >
            {t('shared.actions.resetToDefault')}
          </Button>
        </Box>
      }
      contentSx={{ width: 450 }}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <List>
        <ListItem sx={{ padding: '5px 2px' }}>
          <ListItemText primary={t('settings.modals.tun.fields.stack')} />
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
        </ListItem>

        {tunRuntimeStack && (
          <ListItem sx={{ padding: '0 2px 5px' }}>
            <Typography variant="caption" color="text.secondary">
              {t('settings.modals.tun.messages.activeStack', {
                stack: tunRuntimeStack,
              })}
            </Typography>
          </ListItem>
        )}

        {OS === 'windows' &&
          ['system', 'mixed'].includes(
            (tunRuntimeStack ?? values.stack).toLowerCase(),
          ) && (
            <ListItem sx={{ padding: '0 2px 5px' }}>
              <Typography variant="caption" color="warning.main">
                {t('settings.modals.tun.messages.windowsStackFirewall')}
              </Typography>
            </ListItem>
          )}

        <ListItem sx={{ padding: '5px 2px' }}>
          <ListItemText primary={t('settings.modals.tun.fields.device')} />
          <TextField
            autoComplete="new-password"
            size="small"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 250 }}
            value={values.device}
            placeholder="Mihomo"
            onChange={(e) =>
              setValues((v) => ({ ...v, device: e.target.value }))
            }
          />
        </ListItem>

        <ListItem sx={{ padding: '5px 2px' }}>
          <ListItemText primary={t('settings.modals.tun.fields.autoRoute')} />
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
        </ListItem>

        {OS === 'linux' && (
          <ListItem sx={{ padding: '5px 2px' }}>
            <ListItemText
              primary={t('settings.modals.tun.fields.autoRedirect')}
              sx={{ maxWidth: 'fit-content' }}
            />
            <TooltipIcon
              title={t('settings.modals.tun.tooltips.autoRedirect')}
              sx={{ opacity: values.autoRoute ? 0.7 : 0.3 }}
            />
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
              sx={{ marginLeft: 'auto' }}
            />
          </ListItem>
        )}

        <ListItem sx={{ padding: '5px 2px' }}>
          <ListItemText primary={t('settings.modals.tun.fields.strictRoute')} />
          <ButtonGroup size="small" sx={{ my: '4px' }}>
            {(['auto', 'on', 'off'] as const).map((mode) => (
              <Button
                key={mode}
                variant={values.strictRoute === mode ? 'contained' : 'outlined'}
                onClick={() => setValues((v) => ({ ...v, strictRoute: mode }))}
                sx={{ textTransform: 'capitalize' }}
              >
                {mode}
              </Button>
            ))}
          </ButtonGroup>
        </ListItem>

        <ListItem sx={{ padding: '5px 2px' }}>
          <ListItemText
            primary={t('settings.modals.tun.fields.autoDetectInterface')}
          />
          <Switch
            edge="end"
            checked={values.autoDetectInterface}
            onChange={(_, c) =>
              setValues((v) => ({ ...v, autoDetectInterface: c }))
            }
          />
        </ListItem>

        <ListItem sx={{ padding: '5px 2px' }}>
          <ListItemText primary={t('settings.modals.tun.fields.dnsHijack')} />
          <ButtonGroup size="small" sx={{ my: '4px', marginRight: 1 }}>
            <Button
              variant={values.dnsHijack === 'auto' ? 'contained' : 'outlined'}
              onClick={() => setValues((v) => ({ ...v, dnsHijack: 'auto' }))}
              sx={{ textTransform: 'capitalize' }}
            >
              Auto
            </Button>
          </ButtonGroup>
          <TextField
            autoComplete="new-password"
            size="small"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 180 }}
            value={values.dnsHijack === 'auto' ? '' : values.dnsHijack}
            placeholder={t('settings.modals.tun.tooltips.dnsHijack')}
            onChange={(e) =>
              setValues((v) => ({
                ...v,
                dnsHijack: e.target.value === '' ? 'auto' : e.target.value,
              }))
            }
          />
        </ListItem>

        <ListItem sx={{ padding: '5px 2px' }}>
          <ListItemText primary={t('settings.modals.tun.fields.mtu')} />
          <TextField
            autoComplete="new-password"
            size="small"
            type="number"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 250 }}
            value={values.mtu}
            placeholder="1500"
            onChange={(e) =>
              setValues((v) => ({
                ...v,
                mtu: parseInt(e.target.value),
              }))
            }
          />
        </ListItem>

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
            <ListItem sx={{ padding: '5px 2px' }}>
              <ListItemText
                primary={t('settings.modals.tun.fields.routeExcludeAddress')}
              />
              {modeToggle ? (
                <Box sx={{ marginLeft: 'auto' }}>{modeToggle}</Box>
              ) : null}
            </ListItem>
          )}
        />
      </List>
    </BaseDialog>
  )
}
