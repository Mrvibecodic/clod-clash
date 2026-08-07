import { ContentCopyRounded, SettingsRounded } from '@mui/icons-material'
import { Typography } from '@mui/material'
import { useCallback, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { DialogRef, Switch, TooltipIcon } from '@/components/base'
import { updateLastCheckTime } from '@/hooks/use-update'
import { useVerge } from '@/hooks/use-verge'
import {
  copySupportBundle,
  exitApp,
  exportDiagnosticInfo,
  openAppDir,
  openCoreDir,
  openDevTools,
  openLogsDir,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { checkUpdateSafe as checkUpdate } from '@/services/update'
import { version } from '@root/package.json'

import { BackupViewer } from './mods/backup-viewer'
import { ConfigViewer } from './mods/config-viewer'
import { GuardState } from './mods/guard-state'
import { HotkeyViewer } from './mods/hotkey-viewer'
import { LayoutViewer } from './mods/layout-viewer'
import { LiteModeViewer } from './mods/lite-mode-viewer'
import { MiscViewer } from './mods/misc-viewer'
import { SettingItem, SettingList } from './mods/setting-comp'
import { ThemeViewer } from './mods/theme-viewer'
import { UpdateViewer } from './mods/update-viewer'
import type { SettingVariant } from './setting-variant'

interface Props {
  onError?: (err: Error) => void
  // clod:simple-settings — `core` оставляет только то, без чего пользователь
  // простого режима не сможет попросить помощь: отчёт для поддержки и версию
  // с проверкой обновлений. Всё остальное — `rest`, под «Продвинутыми».
  variant?: SettingVariant
}

const SettingVergeAdvanced = ({ onError, variant = 'all' }: Props) => {
  const { t } = useTranslation()

  const { verge, patchVerge, mutateVerge } = useVerge()
  const { receive_prereleases } = verge ?? {}

  const showCore = variant !== 'rest'
  const showRest = variant !== 'core'

  const configRef = useRef<DialogRef>(null)
  const hotkeyRef = useRef<DialogRef>(null)
  const miscRef = useRef<DialogRef>(null)
  const themeRef = useRef<DialogRef>(null)
  const layoutRef = useRef<DialogRef>(null)
  const updateRef = useRef<DialogRef>(null)
  const backupRef = useRef<DialogRef>(null)
  const liteModeRef = useRef<DialogRef>(null)

  const onCheckUpdate = async () => {
    try {
      const info = await checkUpdate()
      updateLastCheckTime()
      if (!info?.available) {
        showNotice.success(
          'settings.components.verge.advanced.notifications.latestVersion',
        )
      } else {
        updateRef.current?.open()
      }
    } catch (err: any) {
      showNotice.error(err)
    }
  }

  // clod: отчёт для поддержки провайдера — уже отредактированный, в отличие
  // от сырого лога, который пользователь до этого отправлял руками
  const onCopySupportBundle = useCallback(async () => {
    try {
      await copySupportBundle()
      showNotice.success(
        'shared.feedback.notifications.common.supportBundleCopied',
      )
    } catch (error) {
      showNotice.error(error)
    }
  }, [])

  const onExportDiagnosticInfo = useCallback(async () => {
    await exportDiagnosticInfo()
    showNotice.success('shared.feedback.notifications.common.copySuccess', 1000)
  }, [])

  const copyVersion = useCallback(() => {
    navigator.clipboard.writeText(`v${version}`).then(() => {
      showNotice.success(
        'settings.components.verge.advanced.notifications.versionCopied',
        1000,
      )
    })
  }, [])

  return (
    <SettingList
      title={
        variant === 'core'
          ? undefined
          : t('settings.components.verge.advanced.title')
      }
    >
      {showRest && (
        <>
          <ThemeViewer ref={themeRef} />
          <ConfigViewer ref={configRef} />
          <HotkeyViewer ref={hotkeyRef} />
          <MiscViewer ref={miscRef} />
          <LayoutViewer ref={layoutRef} />
          <BackupViewer ref={backupRef} />
          <LiteModeViewer ref={liteModeRef} />
        </>
      )}
      {/* Диалог обновления живёт рядом со строкой версии: её шестерёнка —
          единственное, что его открывает. */}
      {showCore && <UpdateViewer ref={updateRef} />}

      {showRest && (
        <>
          <SettingItem
            onClick={() => backupRef.current?.open()}
            label={t('settings.components.verge.advanced.fields.backupSetting')}
            extra={
              <TooltipIcon
                title={t(
                  'settings.components.verge.advanced.tooltips.backupInfo',
                )}
                sx={{ opacity: '0.7' }}
              />
            }
          />

          <SettingItem
            onClick={() => configRef.current?.open()}
            label={t('settings.components.verge.advanced.fields.runtimeConfig')}
          />

          <SettingItem
            onClick={openAppDir}
            label={t('settings.components.verge.advanced.fields.openConfDir')}
            extra={
              <TooltipIcon
                title={t(
                  'settings.components.verge.advanced.tooltips.openConfDir',
                )}
                sx={{ opacity: '0.7' }}
              />
            }
          />

          <SettingItem
            onClick={openCoreDir}
            label={t('settings.components.verge.advanced.fields.openCoreDir')}
          />

          <SettingItem
            onClick={openLogsDir}
            label={t('settings.components.verge.advanced.fields.openLogsDir')}
          />

          <SettingItem
            onClick={openDevTools}
            label={t('settings.components.verge.advanced.fields.openDevTools')}
          />

          <SettingItem
            label={t(
              'settings.components.verge.advanced.fields.liteModeSettings',
            )}
            extra={
              <TooltipIcon
                title={t(
                  'settings.components.verge.advanced.tooltips.liteMode',
                )}
                sx={{ opacity: '0.7' }}
              />
            }
            onClick={() => liteModeRef.current?.open()}
          />

          <SettingItem
            onClick={() => {
              exitApp()
            }}
            label={t('settings.components.verge.advanced.fields.exit')}
          />

          <SettingItem
            label={t(
              'settings.components.verge.advanced.fields.exportDiagnostics',
            )}
            extra={
              <TooltipIcon
                icon={ContentCopyRounded}
                onClick={onExportDiagnosticInfo}
              />
            }
          ></SettingItem>
          {/* clod:prereleases — единственный канал обновлений у нас общий, а
              выбор пользователя — принимать ли альфы. Живёт рядом с экспортом
              диагностики и версией: всё, что про саму сборку, в одном месте. */}
          <SettingItem
            label={t(
              'settings.components.verge.advanced.fields.receivePrereleases',
            )}
            extra={
              <TooltipIcon
                title={t(
                  'settings.components.verge.advanced.tooltips.receivePrereleases',
                )}
                sx={{ opacity: '0.7' }}
              />
            }
          >
            <GuardState
              value={receive_prereleases ?? true}
              valueProps="checked"
              onCatch={onError}
              onFormat={(_e: any, checked: boolean) => checked}
              onChange={(checked) =>
                mutateVerge({ ...verge, receive_prereleases: checked }, false)
              }
              onGuard={(checked) =>
                patchVerge({ receive_prereleases: checked })
              }
            >
              <Switch edge="end" />
            </GuardState>
          </SettingItem>
        </>
      )}

      {showCore && (
        <>
          {/* clod: то же самое, но для поддержки провайдера: состояние подписки и
          ядра плюс хвосты обоих логов, уже без адресов подписки и токенов */}
          <SettingItem
            label={t('settings.components.verge.advanced.fields.supportBundle')}
            extra={
              <TooltipIcon
                icon={ContentCopyRounded}
                onClick={() => void onCopySupportBundle()}
                title={t(
                  'settings.components.verge.advanced.tooltips.supportBundle',
                )}
              />
            }
          ></SettingItem>

          {/* clod: шестерёнка проверяет обновления приложения — как у строки
          ядра; отдельный пункт «Проверить обновления» из списка убран */}
          <SettingItem
            label={t('settings.components.verge.advanced.fields.vergeVersion')}
            extra={
              <>
                <TooltipIcon
                  icon={SettingsRounded}
                  onClick={() => void onCheckUpdate()}
                  title={t(
                    'settings.components.verge.advanced.fields.checkUpdates',
                  )}
                />
                <TooltipIcon
                  icon={ContentCopyRounded}
                  onClick={copyVersion}
                  title={t(
                    'settings.components.verge.advanced.actions.copyVersion',
                  )}
                />
              </>
            }
          >
            <Typography sx={{ py: '7px', pr: 1 }}>v{version}</Typography>
          </SettingItem>
        </>
      )}
    </SettingList>
  )
}

export default SettingVergeAdvanced
