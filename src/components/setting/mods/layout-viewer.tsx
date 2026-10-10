import { alpha, Box, Button, MenuItem, Select } from '@mui/material'
import { convertFileSrc } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { forwardRef, useEffect, useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  DialogRef,
  FormRow,
  FormSection,
  Switch,
} from '@/components/base'
import { useVerge } from '@/hooks/use-verge'
import { copyIconFile, trayIconPath } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import getSystem from '@/utils/get-system'

import { GuardState } from './guard-state'

const OS = getSystem()

type TrayIconName = 'common' | 'sysproxy' | 'tun'

const TRAY_ICONS = [
  {
    name: 'common',
    field: 'common_tray_icon',
    label: 'settings.components.verge.layout.fields.commonTrayIcon',
    filter: 'Tray Icon Image',
  },
  {
    name: 'sysproxy',
    field: 'sysproxy_tray_icon',
    label: 'settings.components.verge.layout.fields.systemProxyTrayIcon',
    filter: 'Tray Icon Image',
  },
  {
    name: 'tun',
    field: 'tun_tray_icon',
    label: 'settings.components.verge.layout.fields.tunTrayIcon',
    filter: 'Tun Icon Image',
  },
] as const

const SELECT_SX = { width: 180, fontSize: 14 } as const

export const LayoutViewer = forwardRef<DialogRef>((_, ref) => {
  const { t } = useTranslation()
  const { verge, patchVerge, mutateVerge, patchVergeOrRevert } = useVerge()

  const [open, setOpen] = useState(false)
  const [iconPaths, setIconPaths] = useState<Record<TrayIconName, string>>({
    common: '',
    sysproxy: '',
    tun: '',
  })

  useEffect(() => {
    initIconPath()
  }, [])

  async function initIconPath() {
    setIconPaths({
      common: await trayIconPath('common'),
      sysproxy: await trayIconPath('sysproxy'),
      tun: await trayIconPath('tun'),
    })
  }

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }))

  const onSwitchFormat = (_e: any, value: boolean) => value
  const onError = (err: any) => {
    showNotice.error(err)
  }
  const copyPickedIcon = async (path: string, name: TrayIconName) => {
    try {
      await copyIconFile(path, name)
      return true
    } catch (err) {
      onError(err)
      return false
    }
  }
  const onChangeData = (patch: Partial<IVergeConfig>) => {
    mutateVerge({ ...verge, ...patch }, false)
  }

  return (
    <BaseDialog
      open={open}
      title={t('settings.components.verge.layout.title')}
      dividers
      contentSx={{ width: 512 }}
      disableOk
      cancelBtn={t('shared.actions.close')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
    >
      <FormSection
        title={t('settings.components.verge.layout.sections.proxies')}
      />
      <FormRow
        label={t('settings.components.verge.layout.fields.proxyGroupIcon')}
      >
        <GuardState
          value={verge?.enable_group_icon ?? true}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onChange={(e) => onChangeData({ enable_group_icon: e })}
          onGuard={(e) => patchVerge({ enable_group_icon: e })}
        >
          <Switch edge="end" />
        </GuardState>
      </FormRow>

      <FormSection
        title={t('settings.components.verge.layout.sections.notifications')}
      />
      <FormRow
        label={t('settings.components.verge.layout.fields.toastPosition')}
      >
        <GuardState
          value={verge?.notice_position ?? 'top-right'}
          onCatch={onError}
          onFormat={(e: any) => e.target.value}
          onChange={(value) => onChangeData({ notice_position: value })}
          onGuard={(value) => patchVerge({ notice_position: value })}
        >
          <Select size="small" sx={SELECT_SX}>
            <MenuItem value="top-right">
              {t(
                'settings.components.verge.layout.options.toastPosition.topRight',
              )}
            </MenuItem>
            <MenuItem value="top-left">
              {t(
                'settings.components.verge.layout.options.toastPosition.topLeft',
              )}
            </MenuItem>
            <MenuItem value="bottom-right">
              {t(
                'settings.components.verge.layout.options.toastPosition.bottomRight',
              )}
            </MenuItem>
            <MenuItem value="bottom-left">
              {t(
                'settings.components.verge.layout.options.toastPosition.bottomLeft',
              )}
            </MenuItem>
          </Select>
        </GuardState>
      </FormRow>

      <FormSection
        title={t('settings.components.verge.layout.sections.tray')}
      />
      {OS === 'macos' && (
        <FormRow label={t('settings.components.verge.layout.fields.trayIcon')}>
          <GuardState
            value={verge?.tray_icon ?? 'monochrome'}
            onCatch={onError}
            onFormat={(e: any) => e.target.value}
            onChange={(e) => onChangeData({ tray_icon: e })}
            onGuard={(e) => patchVerge({ tray_icon: e })}
          >
            <Select size="small" sx={SELECT_SX}>
              <MenuItem value="monochrome">
                {t('settings.components.verge.layout.options.icon.monochrome')}
              </MenuItem>
              <MenuItem value="colorful">
                {t('settings.components.verge.layout.options.icon.colorful')}
              </MenuItem>
            </Select>
          </GuardState>
        </FormRow>
      )}
      {OS === 'macos' && (
        <FormRow
          label={t('settings.components.verge.layout.fields.enableTraySpeed')}
        >
          <GuardState
            value={verge?.enable_tray_speed ?? false}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onChange={(e) => onChangeData({ enable_tray_speed: e })}
            onGuard={(e) => patchVerge({ enable_tray_speed: e })}
          >
            <Switch edge="end" />
          </GuardState>
        </FormRow>
      )}
      <FormRow
        label={t(
          'settings.components.verge.layout.fields.proxyGroupsDisplayMode',
        )}
      >
        <GuardState
          value={verge?.tray_proxy_groups_display_mode ?? 'default'}
          onCatch={onError}
          onFormat={(e: any) => e.target.value}
          onChange={(value) =>
            onChangeData({ tray_proxy_groups_display_mode: value })
          }
          onGuard={(value) =>
            patchVerge({ tray_proxy_groups_display_mode: value })
          }
        >
          <Select size="small" sx={SELECT_SX}>
            <MenuItem value="default">
              {t(
                'settings.components.verge.layout.options.proxyGroupsDisplayMode.default',
              )}
            </MenuItem>
            <MenuItem value="inline">
              {t(
                'settings.components.verge.layout.options.proxyGroupsDisplayMode.inline',
              )}
            </MenuItem>
            <MenuItem value="disable">
              {t(
                'settings.components.verge.layout.options.proxyGroupsDisplayMode.disable',
              )}
            </MenuItem>
          </Select>
        </GuardState>
      </FormRow>
      <FormRow
        label={t(
          'settings.components.verge.layout.fields.showOutboundModesInline',
        )}
      >
        <GuardState
          value={verge?.tray_inline_outbound_modes ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onChange={(e) => onChangeData({ tray_inline_outbound_modes: e })}
          onGuard={(e) => patchVerge({ tray_inline_outbound_modes: e })}
        >
          <Switch edge="end" />
        </GuardState>
      </FormRow>

      <FormSection
        title={t('settings.components.verge.layout.sections.trayIcons')}
        hint="PNG, ICO"
      />
      {TRAY_ICONS.map(({ name, field, label, filter }) => {
        const custom = Boolean(verge?.[field])
        return (
          <FormRow key={name} label={t(label)}>
            <Box
              sx={({ palette }) => ({
                width: 28,
                height: 28,
                display: 'grid',
                placeItems: 'center',
                borderRadius: '6px',
                bgcolor: alpha(palette.text.primary, 0.07),
                color: 'text.disabled',
                fontSize: 13,
              })}
            >
              {custom && iconPaths[name] ? (
                <img
                  alt=""
                  width={20}
                  height={20}
                  style={{ objectFit: 'contain' }}
                  src={convertFileSrc(iconPaths[name])}
                />
              ) : (
                '—'
              )}
            </Box>
            <Button
              variant="outlined"
              size="small"
              onClick={async () => {
                if (custom) {
                  await patchVergeOrRevert({ [field]: false })
                } else {
                  const selected = await openDialog({
                    directory: false,
                    multiple: false,
                    filters: [{ name: filter, extensions: ['png', 'ico'] }],
                  })
                  if (selected && (await copyPickedIcon(`${selected}`, name))) {
                    await initIconPath()
                    await patchVergeOrRevert({ [field]: true })
                  }
                }
              }}
            >
              {custom
                ? t('shared.actions.clear')
                : t('settings.components.verge.basic.actions.browse')}
            </Button>
          </FormRow>
        )
      })}
    </BaseDialog>
  )
})
