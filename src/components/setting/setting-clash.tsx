import { LanRounded, SettingsRounded } from '@mui/icons-material'
import { MenuItem, Select, TextField, Tooltip, Typography } from '@mui/material'
import { invoke } from '@tauri-apps/api/core'
import { useLockFn } from 'ahooks'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { updateGeo, type LogLevel } from 'tauri-plugin-mihomo-api'

import { type DialogRef, Switch, TooltipIcon } from '@/components/base'
import { useClash } from '@/hooks/use-clash'
import { useClashLog } from '@/hooks/use-clash-log'
import { useProfiles } from '@/hooks/use-profiles'
import { useVerge } from '@/hooks/use-verge'
import { invoke_uwp_tool, patchClashMode } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import getSystem from '@/utils/get-system'

import { ClashCoreViewer } from './mods/clash-core-viewer'
import { ClashPortViewer } from './mods/clash-port-viewer'
import { ControllerViewer } from './mods/controller-viewer'
import { DnsViewer } from './mods/dns-viewer'
import { HeaderConfiguration } from './mods/external-controller-cors'
import { GuardState } from './mods/guard-state'
import { ManagedCoreViewer } from './mods/managed-core-viewer'
import { NetworkInterfaceViewer } from './mods/network-interface-viewer'
import { SettingItem, SettingList } from './mods/setting-comp'
import { TunnelsViewer } from './mods/tunnels-viewer'
import { WebUIViewer } from './mods/web-ui-viewer'

const isWIN = getSystem() === 'windows'

const unifiedDelayOf = (choice: string): boolean | 'auto' =>
  choice === 'on' ? true : choice === 'off' ? false : 'auto'

interface Props {
  onError: (err: Error) => void
}

const SettingClash = ({ onError }: Props) => {
  const { t } = useTranslation()

  const { clash, version, mutateClash, patchClash } = useClash()
  const { verge, patchVerge } = useVerge()
  const [, setClashLog] = useClashLog()

  const {
    ipv6,
    mode,
    'allow-lan': allowLan,
    'log-level': logLevel,
    'unified-delay': unifiedDelay,
  } = clash ?? {}

  const { current } = useProfiles()
  const modeLocked = Boolean(current?.lock_mode)
  const normalizedMode = mode?.toLowerCase()
  const routingMode =
    normalizedMode === 'global' || normalizedMode === 'direct'
      ? normalizedMode
      : 'rule'

  const { verge_mixed_port } = verge ?? {}

  const [dnsSettingsEnabled, setDnsSettingsEnabled] = useState(() => {
    return verge?.enable_dns_settings ?? false
  })

  const webRef = useRef<DialogRef>(null)
  const portRef = useRef<DialogRef>(null)
  const ctrlRef = useRef<DialogRef>(null)
  const coreRef = useRef<DialogRef>(null)
  const managedCoreRef = useRef<DialogRef>(null)
  const networkRef = useRef<DialogRef>(null)
  const dnsRef = useRef<DialogRef>(null)
  const corsRef = useRef<DialogRef>(null)
  const tunnelRef = useRef<DialogRef>(null)

  const onSwitchFormat = (_e: any, value: boolean) => value
  const onChangeData = (patch: Partial<IConfigData>) => {
    mutateClash((old) => ({ ...old!, ...patch }), false)
  }
  const onUpdateGeo = async () => {
    try {
      await updateGeo()
      showNotice.success('settings.feedback.notifications.clash.geoDataUpdated')
    } catch (err: any) {
      showNotice.error(err)
    }
  }

  const handleDnsToggle = useLockFn(async (enable: boolean) => {
    const previous = dnsSettingsEnabled
    let settingStored = false

    setDnsSettingsEnabled(enable)
    try {
      await patchVerge({ enable_dns_settings: enable })
      settingStored = true
      await invoke('apply_dns_config', { apply: enable })
      setTimeout(() => {
        mutateClash()
      }, 500)
    } catch (err: any) {
      showNotice.error(err)

      if (!settingStored) {
        setDnsSettingsEnabled(previous)
        return
      }

      try {
        await patchVerge({ enable_dns_settings: previous })
        setDnsSettingsEnabled(previous)
      } catch (revertErr) {
        setDnsSettingsEnabled(enable)
        showNotice.error(revertErr)
      }
    }
  })

  return (
    <SettingList title={t('settings.sections.clash.title')}>
      <WebUIViewer ref={webRef} />
      <ClashPortViewer ref={portRef} />
      <ControllerViewer ref={ctrlRef} />
      <ClashCoreViewer ref={coreRef} />
      <NetworkInterfaceViewer ref={networkRef} />
      <DnsViewer ref={dnsRef} />
      <HeaderConfiguration ref={corsRef} />
      <TunnelsViewer ref={tunnelRef} />
      {modeLocked ? (
        <SettingItem
          label={t('settings.sections.clash.form.fields.routingMode')}
          extra={
            <TooltipIcon
              title={t('home.components.modeStatus.lockedHint')}
              color={'inherit'}
            />
          }
        >
          <Typography variant="body2" color="text.secondary">
            {t(`home.components.clashMode.labels.${routingMode}`)}
          </Typography>
        </SettingItem>
      ) : (
        <SettingItem
          label={t('settings.sections.clash.form.fields.routingMode')}
          extra={
            <TooltipIcon
              title={t('settings.sections.clash.form.tooltips.routingMode')}
              color={'inherit'}
            />
          }
        >
          <GuardState
            value={routingMode}
            onCatch={onError}
            onFormat={(e: any) => e.target.value}
            onChange={(mode) => onChangeData({ mode })}
            onGuard={(mode) => patchClashMode(mode)}
          >
            <Select size="small" sx={{ width: 140, '> div': { py: '7.5px' } }}>
              <MenuItem value="rule">
                {t('home.components.clashMode.labels.rule')}
              </MenuItem>
              <MenuItem value="global">
                {t('home.components.clashMode.labels.global')}
              </MenuItem>
              <MenuItem value="direct">
                {t('home.components.clashMode.labels.direct')}
              </MenuItem>
            </Select>
          </GuardState>
        </SettingItem>
      )}

      <SettingItem
        label={t('settings.sections.clash.form.fields.allowLan')}
        extra={
          <TooltipIcon
            title={t('settings.sections.clash.form.tooltips.networkInterface')}
            color={'inherit'}
            icon={LanRounded}
            onClick={() => {
              networkRef.current?.open()
            }}
          />
        }
      >
        <GuardState
          value={allowLan ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onChange={(e) => onChangeData({ 'allow-lan': e })}
          onGuard={(e) => patchClash({ 'allow-lan': e })}
        >
          <Switch edge="end" />
        </GuardState>
      </SettingItem>

      <SettingItem
        label={t('settings.sections.clash.form.fields.ipv6')}
        extra={
          <TooltipIcon
            title={t('settings.sections.clash.form.tooltips.ipv6')}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <Switch edge="end" checked={ipv6 ?? false} disabled />
      </SettingItem>

      <SettingItem
        label={t('settings.sections.clash.form.fields.dnsOverwrite')}
        extra={
          <TooltipIcon
            icon={SettingsRounded}
            onClick={() => dnsRef.current?.open()}
          />
        }
      >
        <Tooltip
          title={t('settings.sections.clash.form.tooltips.dnsOverwrite')}
          placement="top"
        >
          <Switch
            edge="end"
            checked={dnsSettingsEnabled}
            onChange={(_, checked) => handleDnsToggle(checked)}
          />
        </Tooltip>
      </SettingItem>

      <SettingItem
        label={t('settings.sections.clash.form.fields.unifiedDelay')}
        extra={
          <TooltipIcon
            title={t('settings.sections.clash.form.tooltips.unifiedDelay')}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <GuardState
          value={
            unifiedDelay === true
              ? 'on'
              : unifiedDelay === false
                ? 'off'
                : 'auto'
          }
          onCatch={onError}
          onFormat={(e: any) => e.target.value}
          onChange={(e) => onChangeData({ 'unified-delay': unifiedDelayOf(e) })}
          onGuard={(e) => patchClash({ 'unified-delay': unifiedDelayOf(e) })}
        >
          <Select size="small" sx={{ width: 160, '> div': { py: '7.5px' } }}>
            <MenuItem value="auto">
              {t('settings.sections.clash.form.options.unifiedDelay.auto')}
            </MenuItem>
            <MenuItem value="on">
              {t('settings.sections.clash.form.options.unifiedDelay.on')}
            </MenuItem>
            <MenuItem value="off">
              {t('settings.sections.clash.form.options.unifiedDelay.off')}
            </MenuItem>
          </Select>
        </GuardState>
      </SettingItem>

      <SettingItem
        label={t('settings.sections.clash.form.fields.logLevel')}
        extra={
          <TooltipIcon
            title={t('settings.sections.clash.form.tooltips.logLevel')}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <GuardState
          value={logLevel === 'warn' ? 'warning' : (logLevel ?? 'auto')}
          onCatch={onError}
          onFormat={(e: any) => e.target.value}
          onChange={(e) => onChangeData({ 'log-level': e })}
          onGuard={(e) => {
            if (e !== 'auto') {
              setClashLog((pre) => ({
                ...pre!,
                logLevel: e.toUpperCase() as LogLevel,
              }))
            }
            return patchClash({ 'log-level': e })
          }}
        >
          <Select size="small" sx={{ width: 160, '> div': { py: '7.5px' } }}>
            <MenuItem value="auto">
              {t('settings.sections.clash.form.options.logLevel.auto')}
            </MenuItem>
            <MenuItem value="debug">
              {t('settings.sections.clash.form.options.logLevel.debug')}
            </MenuItem>
            <MenuItem value="info">
              {t('settings.sections.clash.form.options.logLevel.info')}
            </MenuItem>
            <MenuItem value="warning">
              {t('settings.sections.clash.form.options.logLevel.warning')}
            </MenuItem>
            <MenuItem value="error">
              {t('settings.sections.clash.form.options.logLevel.error')}
            </MenuItem>
            <MenuItem value="silent">
              {t('settings.sections.clash.form.options.logLevel.silent')}
            </MenuItem>
          </Select>
        </GuardState>
      </SettingItem>

      <SettingItem label={t('settings.sections.clash.form.fields.portConfig')}>
        <TextField
          autoComplete="new-password"
          disabled={false}
          size="small"
          value={verge_mixed_port ?? 7897}
          sx={{ width: 100, input: { py: '7.5px', cursor: 'pointer' } }}
          onClick={(e) => {
            portRef.current?.open()
            ;(e.target as HTMLElement).blur()
          }}
        />
      </SettingItem>

      <SettingItem
        label={t('settings.sections.clash.form.fields.external')}
        extra={
          <TooltipIcon
            title={t('settings.sections.externalCors.tooltips.open')}
            icon={SettingsRounded}
            onClick={(e) => {
              e.stopPropagation()
              corsRef.current?.open()
            }}
          />
        }
        onClick={() => {
          ctrlRef.current?.open()
        }}
      />

      <SettingItem
        onClick={() => webRef.current?.open()}
        label={t('settings.sections.clash.form.fields.webUI')}
      />

      <SettingItem
        label={t('settings.sections.clash.form.fields.clashCore')}
        extra={
          <TooltipIcon
            icon={SettingsRounded}
            onClick={() => coreRef.current?.open()}
          />
        }
      >
        <Typography sx={{ py: '7px', pr: 1 }}>{version}</Typography>
      </SettingItem>

      <SettingItem
        onClick={() => managedCoreRef.current?.open()}
        label={t('settings.modals.managedCore.entry')}
      />
      <ManagedCoreViewer ref={managedCoreRef} />

      {isWIN && (
        <SettingItem
          onClick={invoke_uwp_tool}
          label={t('settings.sections.clash.form.fields.openUwpTool')}
          extra={
            <TooltipIcon
              title={t('settings.sections.clash.form.tooltips.openUwpTool')}
              sx={{ opacity: '0.7' }}
            />
          }
        />
      )}

      <SettingItem
        onClick={onUpdateGeo}
        label={t('settings.sections.clash.form.fields.updateGeoData')}
      />

      <SettingItem
        label={t('settings.sections.clash.form.fields.tunnels.title')}
        onClick={() => tunnelRef.current?.open()}
      />
    </SettingList>
  )
}

export default SettingClash
