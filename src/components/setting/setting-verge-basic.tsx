import { ContentCopyRounded } from '@mui/icons-material'
import { Box, Button, Input, MenuItem, Select } from '@mui/material'
import { open } from '@tauri-apps/plugin-dialog'
import { useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import useSWR from 'swr'

import { type DialogRef, Switch, TooltipIcon } from '@/components/base'
import { useSimpleMode } from '@/hooks/use-simple-mode'
import { useVerge } from '@/hooks/use-verge'
import { navigationItems } from '@/pages/_navigation-meta'
import { copyClashEnv, getDeviceIdentity } from '@/services/cmds'
import { supportedLanguages } from '@/services/i18n'
import { showNotice } from '@/services/notice-service'
import getSystem from '@/utils/get-system'

import { BackupViewer } from './mods/backup-viewer'
import { ConfigViewer } from './mods/config-viewer'
import { GuardState } from './mods/guard-state'
import { HotkeyViewer } from './mods/hotkey-viewer'
import { LayoutViewer } from './mods/layout-viewer'
import { MiscViewer } from './mods/misc-viewer'
import { SettingItem, SettingList } from './mods/setting-comp'
import { ThemeModeSwitch } from './mods/theme-mode-switch'
import { ThemeViewer } from './mods/theme-viewer'
import { UpdateViewer } from './mods/update-viewer'
import type { SettingVariant } from './setting-variant'

interface Props {
  onError?: (err: Error) => void
  variant?: SettingVariant
}

const OS = getSystem()

const languageOptions = supportedLanguages.map((code) => {
  const labels: { [key: string]: string } = {
    en: 'English',
    ru: 'Русский',
    zh: '中文',
    fa: 'فارسی',
    tt: 'Татар',
    id: 'Bahasa Indonesia',
    ar: 'العربية',
    ko: '한국어',
    tr: 'Türkçe',
    de: 'Deutsch',
    es: 'Español',
    jp: '日本語',
    zhtw: '繁體中文',
  }
  const label = labels[code] || code
  return { code, label }
})

const SettingVergeBasic = ({ onError, variant = 'all' }: Props) => {
  const { t } = useTranslation()

  const showCore = variant !== 'rest'
  const showRest = variant !== 'core'

  const { verge, patchVerge, mutateVerge } = useVerge()
  const {
    theme_mode,
    language,
    tray_event,
    env_type,
    startup_script,
    start_page,
    enable_sub_notifications,
    enable_hwid,
  } = verge ?? {}
  // clod: показываем в тултипе фактические значения, а не описание полей
  const { data: identity, mutate: mutateIdentity } = useSWR(
    'getDeviceIdentity',
    getDeviceIdentity,
  )
  const { simpleMode, setSimpleMode } = useSimpleMode()
  const configRef = useRef<DialogRef>(null)
  const hotkeyRef = useRef<DialogRef>(null)
  const miscRef = useRef<DialogRef>(null)
  const themeRef = useRef<DialogRef>(null)
  const layoutRef = useRef<DialogRef>(null)
  const updateRef = useRef<DialogRef>(null)
  const backupRef = useRef<DialogRef>(null)

  const onChangeData = (patch: any) => {
    mutateVerge({ ...verge, ...patch }, false)
  }

  const onCopyClashEnv = useCallback(async () => {
    await copyClashEnv()
    showNotice.success('shared.feedback.notifications.common.copySuccess', 1000)
  }, [])

  return (
    <SettingList
      title={
        // clod:simple-settings — под «Продвинутыми» это уже не «Основные»:
        // группа называется по тому, что в ней осталось, иначе внутри блока
        // висел бы второй заголовок «Основные» рядом с первым.
        variant === 'rest'
          ? t('settings.sections.advancedGroup.interface')
          : t('settings.components.verge.basic.title')
      }
    >
      {showRest && (
        <>
          <ThemeViewer ref={themeRef} />
          <ConfigViewer ref={configRef} />
          <HotkeyViewer ref={hotkeyRef} />
          <MiscViewer ref={miscRef} />
          <LayoutViewer ref={layoutRef} />
          <UpdateViewer ref={updateRef} />
          <BackupViewer ref={backupRef} />
        </>
      )}

      {showCore && (
        <>
          {/* clod: простое и повседневное — сверху: язык и тема первыми */}
          <SettingItem
            label={t('settings.components.verge.basic.fields.language')}
          >
            <GuardState
              value={language ?? 'en'}
              onCatch={onError}
              onFormat={(e: any) => e.target.value}
              onChange={(e) => onChangeData({ language: e })}
              onGuard={(e) => patchVerge({ language: e })}
            >
              <Select
                size="small"
                sx={{ width: 110, '> div': { py: '7.5px' } }}
              >
                {languageOptions.map(({ code, label }) => (
                  <MenuItem key={code} value={code}>
                    {label}
                  </MenuItem>
                ))}
              </Select>
            </GuardState>
          </SettingItem>

          <SettingItem
            label={t('settings.components.verge.basic.fields.themeMode')}
          >
            <GuardState
              value={theme_mode}
              onCatch={onError}
              onChange={(e) => onChangeData({ theme_mode: e })}
              onGuard={(e) => patchVerge({ theme_mode: e })}
            >
              <ThemeModeSwitch />
            </GuardState>
          </SettingItem>

          {/* clod: advanced mode is an honest switch, not a hidden gesture */}
          <SettingItem
            label={t('settings.components.verge.basic.fields.advancedMode')}
          >
            <GuardState
              value={!simpleMode}
              valueProps="checked"
              onCatch={onError}
              onFormat={(_e: any, checked: boolean) => checked}
              onChange={(advanced) => onChangeData({ simple_mode: !advanced })}
              onGuard={(advanced) => setSimpleMode(!advanced)}
            >
              <Switch edge="end" />
            </GuardState>
          </SettingItem>
        </>
      )}

      {/* clod:F7 — the user's global switch for the subscription
          expiry/traffic notifications; off wins over the panel. */}
      {showRest && (
        <SettingItem
          label={t('settings.components.verge.basic.fields.subNotifications')}
          extra={
            <TooltipIcon
              title={t(
                'settings.components.verge.basic.hints.subNotifications',
              )}
            />
          }
        >
          <GuardState
            value={enable_sub_notifications ?? true}
            valueProps="checked"
            onCatch={onError}
            onFormat={(_e: any, checked: boolean) => checked}
            onChange={(checked) =>
              onChangeData({ enable_sub_notifications: checked })
            }
            onGuard={(checked) =>
              patchVerge({ enable_sub_notifications: checked })
            }
          >
            <Switch edge="end" />
          </GuardState>
        </SettingItem>
      )}

      {/* clod: отправку отпечатка устройства пользователь должен видеть и
          уметь выключить. Тултип показывает ровно те значения, которые уходят
          в панель, — иначе «идентификация устройства» остаётся обещанием. */}
      {showCore && (
        <SettingItem
          label={t('settings.components.verge.basic.fields.deviceIdentity')}
          extra={
            <TooltipIcon
              title={
                <Box sx={{ whiteSpace: 'pre-line' }}>
                  {[
                    t('settings.components.verge.basic.hints.deviceIdentity'),
                    // Все четыре x-* уходят вместе с отпечатком: нет hwid —
                    // нет ни одного из них. User-Agent отправляется всегда.
                    identity?.hwid ? `x-hwid: ${identity.hwid}` : null,
                    identity?.hwid ? `x-device-os: ${identity.os}` : null,
                    identity?.hwid ? `x-ver-os: ${identity.os_version}` : null,
                    identity?.hwid ? `x-device-model: ${identity.model}` : null,
                    identity ? `User-Agent: ${identity.user_agent}` : null,
                  ]
                    .filter(Boolean)
                    .join('\n')}
                </Box>
              }
            />
          }
        >
          <GuardState
            value={enable_hwid ?? true}
            valueProps="checked"
            onCatch={onError}
            onFormat={(_e: any, checked: boolean) => checked}
            onChange={(checked) => onChangeData({ enable_hwid: checked })}
            onGuard={async (checked) => {
              await patchVerge({ enable_hwid: checked })
              // The id is computed lazily, so the tooltip only tells the truth
              // once the backend has been asked again.
              await mutateIdentity()
            }}
          >
            <Switch edge="end" />
          </GuardState>
        </SettingItem>
      )}

      {showRest && (
        <>
          {OS !== 'linux' && (
            <SettingItem
              label={t('settings.components.verge.basic.fields.trayClickEvent')}
            >
              <GuardState
                value={tray_event ?? 'main_window'}
                onCatch={onError}
                onFormat={(e: any) => e.target.value}
                onChange={(e) => onChangeData({ tray_event: e })}
                onGuard={(e) => patchVerge({ tray_event: e })}
              >
                <Select
                  size="small"
                  sx={{ width: 140, '> div': { py: '7.5px' } }}
                >
                  <MenuItem value="main_window">
                    {t(
                      'settings.components.verge.basic.trayOptions.showMainWindow',
                    )}
                  </MenuItem>
                  <MenuItem value="tray_menu">
                    {t(
                      'settings.components.verge.basic.trayOptions.showTrayMenu',
                    )}
                  </MenuItem>
                  <MenuItem value="system_proxy">
                    {t('settings.sections.system.toggles.systemProxy')}
                  </MenuItem>
                  <MenuItem value="tun_mode">
                    {t('settings.sections.system.toggles.tunMode')}
                  </MenuItem>
                  <MenuItem value="disable">
                    {t('settings.components.verge.basic.trayOptions.disable')}
                  </MenuItem>
                </Select>
              </GuardState>
            </SettingItem>
          )}

          <SettingItem
            label={t('settings.components.verge.basic.fields.startPage')}
          >
            <GuardState
              value={start_page ?? '/'}
              onCatch={onError}
              onFormat={(e: any) => e.target.value}
              onChange={(e) => onChangeData({ start_page: e })}
              onGuard={(e) => patchVerge({ start_page: e })}
            >
              <Select
                size="small"
                sx={{ width: 140, '> div': { py: '7.5px' } }}
              >
                {Object.values(navigationItems).map((page) => {
                  return (
                    <MenuItem key={page.path} value={page.path}>
                      {t(page.label)}
                    </MenuItem>
                  )
                })}
              </Select>
            </GuardState>
          </SettingItem>

          <SettingItem
            onClick={() => themeRef.current?.open()}
            label={t('settings.components.verge.basic.fields.themeSetting')}
          />

          <SettingItem
            onClick={() => layoutRef.current?.open()}
            label={t('settings.components.verge.basic.fields.layoutSetting')}
          />

          <SettingItem
            onClick={() => miscRef.current?.open()}
            label={t('settings.components.verge.basic.fields.misc')}
          />

          <SettingItem
            onClick={() => hotkeyRef.current?.open()}
            label={t('settings.components.verge.basic.fields.hotkeySetting')}
          />

          {/* clod: гиковское — в самый низ секции */}
          <SettingItem
            label={t('settings.components.verge.basic.fields.copyEnvType')}
            extra={
              <TooltipIcon icon={ContentCopyRounded} onClick={onCopyClashEnv} />
            }
          >
            <GuardState
              value={env_type ?? (OS === 'windows' ? 'powershell' : 'bash')}
              onCatch={onError}
              onFormat={(e: any) => e.target.value}
              onChange={(e) => onChangeData({ env_type: e })}
              onGuard={(e) => patchVerge({ env_type: e })}
            >
              <Select
                size="small"
                sx={{ width: 140, '> div': { py: '7.5px' } }}
              >
                <MenuItem value="bash">Bash</MenuItem>
                <MenuItem value="fish">Fish</MenuItem>
                <MenuItem value="nushell">Nushell</MenuItem>
                <MenuItem value="cmd">CMD</MenuItem>
                <MenuItem value="powershell">PowerShell</MenuItem>
              </Select>
            </GuardState>
          </SettingItem>

          <SettingItem
            label={t('settings.components.verge.basic.fields.startupScript')}
          >
            <GuardState
              value={startup_script ?? ''}
              onCatch={onError}
              onFormat={(e: any) => e.target.value}
              onChange={(e) => onChangeData({ startup_script: e })}
              onGuard={(e) => patchVerge({ startup_script: e })}
            >
              <Input
                value={startup_script}
                disabled
                disableUnderline
                sx={{ width: 230 }}
                endAdornment={
                  <>
                    <Button
                      onClick={async () => {
                        const selected = await open({
                          directory: false,
                          multiple: false,
                          filters: [
                            {
                              name: 'Shell Script',
                              extensions: ['sh', 'bat', 'ps1'],
                            },
                          ],
                        })
                        if (selected) {
                          onChangeData({ startup_script: `${selected}` })
                          patchVerge({ startup_script: `${selected}` })
                        }
                      }}
                    >
                      {t('settings.components.verge.basic.actions.browse')}
                    </Button>
                    {startup_script && (
                      <Button
                        onClick={async () => {
                          onChangeData({ startup_script: '' })
                          patchVerge({ startup_script: '' })
                        }}
                      >
                        {t('shared.actions.clear')}
                      </Button>
                    )}
                  </>
                }
              ></Input>
            </GuardState>
          </SettingItem>
        </>
      )}
    </SettingList>
  )
}

export default SettingVergeBasic
