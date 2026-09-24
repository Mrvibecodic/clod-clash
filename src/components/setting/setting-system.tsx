import React from 'react'
import { useTranslation } from 'react-i18next'

import { Switch, TooltipIcon } from '@/components/base'
import ProxyControlSwitches from '@/components/shared/proxy-control-switches'
import { useVerge } from '@/hooks/use-verge'
import getSystem from '@/utils/get-system'

import { GuardState } from './mods/guard-state'
import { SettingList, SettingItem } from './mods/setting-comp'

const OS = getSystem()

interface Props {
  onError?: (err: Error) => void
}

const SettingSystem = ({ onError }: Props) => {
  const { t } = useTranslation()

  const { verge, mutateVerge, patchVerge } = useVerge()

  const {
    enable_auto_launch,
    enable_silent_start,
    connect_on_launch,
    enable_dns_override,
  } = verge ?? {}

  const onSwitchFormat = (
    _e: React.ChangeEvent<HTMLInputElement>,
    value: boolean,
  ) => value
  const onChangeData = (patch: Partial<IVergeConfig>) => {
    mutateVerge({ ...verge, ...patch }, false)
  }

  return (
    <SettingList title={t('settings.sections.system.title')}>
      <ProxyControlSwitches target="tun" onError={onError} />

      <ProxyControlSwitches target="sysproxy" onError={onError} />

      {/* clod: подключение — действие пользователя. Раньше оно восстанавливалось
          само по флагам «поднято сейчас», переживавшим перезапуск, и человек
          обнаруживал в настройках Windows прокси, которого не включал. Теперь
          автомат существует, но только как явно включённая настройка. */}
      <SettingItem
        label={t('settings.sections.system.fields.connectOnLaunch')}
        extra={
          <TooltipIcon
            title={t('settings.sections.system.tooltips.connectOnLaunch')}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <GuardState
          value={connect_on_launch ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onChange={(e) => onChangeData({ connect_on_launch: e })}
          onGuard={(e) => patchVerge({ connect_on_launch: e })}
        >
          <Switch edge="end" />
        </GuardState>
      </SettingItem>

      <SettingItem label={t('settings.sections.system.fields.autoLaunch')}>
        <GuardState
          value={enable_auto_launch ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onChange={(e) => {
            onChangeData({ enable_auto_launch: e })
          }}
          onGuard={(e) => patchVerge({ enable_auto_launch: e })}
        >
          <Switch edge="end" />
        </GuardState>
      </SettingItem>

      {OS === 'macos' && (
        <SettingItem
          label={t('settings.sections.system.fields.dnsOverride')}
          extra={
            <TooltipIcon
              title={t('settings.sections.system.tooltips.dnsOverride')}
              sx={{ opacity: '0.7' }}
            />
          }
        >
          <GuardState
            value={enable_dns_override ?? true}
            valueProps="checked"
            onCatch={onError}
            onFormat={onSwitchFormat}
            onChange={(e) => onChangeData({ enable_dns_override: e })}
            onGuard={(e) => patchVerge({ enable_dns_override: e })}
          >
            <Switch edge="end" />
          </GuardState>
        </SettingItem>
      )}

      <SettingItem
        label={t('settings.sections.system.fields.silentStart')}
        extra={
          <TooltipIcon
            title={t('settings.sections.system.tooltips.silentStart')}
            sx={{ opacity: '0.7' }}
          />
        }
      >
        <GuardState
          value={enable_silent_start ?? false}
          valueProps="checked"
          onCatch={onError}
          onFormat={onSwitchFormat}
          onChange={(e) => onChangeData({ enable_silent_start: e })}
          onGuard={(e) => patchVerge({ enable_silent_start: e })}
        >
          <Switch edge="end" />
        </GuardState>
      </SettingItem>
    </SettingList>
  )
}

export default SettingSystem
