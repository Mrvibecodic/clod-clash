import {
  RestartAltRounded,
  SwitchAccessShortcutRounded,
} from '@mui/icons-material'
import {
  Box,
  Button,
  Chip,
  CircularProgress,
  List,
  ListItemButton,
  ListItemText,
  Typography,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import type { Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { closeAllConnections, upgradeCore } from 'tauri-plugin-mihomo-api'

import { BaseDialog, DialogRef } from '@/components/base'
import { useClash, useClashInfo } from '@/hooks/use-clash'
import { useVerge } from '@/hooks/use-verge'
import {
  changeClashCore,
  getCoreUpdaterStatus,
  repinCoreBinaries,
  restartCore,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import getSystem from '@/utils/get-system'

// Оба ядра лежат в установщике и работают и через службу, и своим процессом.
// verge-mihomo — стоковый MetaCubeX (по умолчанию), verge-mihomo-alpha — Clod
// Core (наш форк mihomo с патчами); имена файлов исторические, служба знает
// только их.
const VALID_CORE = [
  {
    name: 'Mihomo',
    core: 'verge-mihomo',
    chipKey: 'settings.modals.clashCore.variants.release',
  },
  {
    name: 'Clod Core',
    core: 'verge-mihomo-alpha',
    chipKey: 'settings.modals.clashCore.variants.alpha',
  },
]

export function ClashCoreViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()

  const { verge, mutateVerge } = useVerge()
  const { mutateVersion } = useClash()
  const { invalidateClashConfig } = useClashInfo()

  const [open, setOpen] = useState(false)
  const [upgrading, setUpgrading] = useState(false)
  const [restarting, setRestarting] = useState(false)
  const [changingCore, setChangingCore] = useState<string | null>(null)

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }))

  const { clash_core = 'verge-mihomo' } = verge ?? {}

  const onCoreChange = useLockFn(async (core: string) => {
    if (core === clash_core) return

    try {
      setChangingCore(core)
      void closeAllConnections().catch(() => undefined)
      const errorMsg = await changeClashCore(core)
      mutateVerge()
      if (errorMsg) return

      await new Promise((resolve) => setTimeout(resolve, 500))
      invalidateClashConfig()
      mutateVersion()
    } catch (err) {
      showNotice.error(err)
    } finally {
      setChangingCore(null)
    }
  })

  const onRestart = useLockFn(async () => {
    try {
      setRestarting(true)
      await restartCore()
      showNotice.success(
        t('settings.feedback.notifications.clash.restartSuccess'),
      )
      setRestarting(false)
    } catch (err) {
      setRestarting(false)
      showNotice.error(err)
    }
  })

  // Ядро обновляет себя само (/upgrade): каждое из двух ходит на свой
  // источник — Clod Core на релизы clod-core, Mihomo на MetaCubeX — и
  // подменяет свой файл в папке программы. На Windows только под службой:
  // своим процессом ядро после подмены перезапускает себя дочерним процессом,
  // приложение видит выход и поднимает второе ядро на том же порту (на
  // macOS/Linux ядро делает exec — процесс и PID те же). Управляемый
  // обновитель здесь не участвует: он подставил бы стоковое ядро из папки
  // пользователя поверх выбранного.
  const upgradeThroughCore = async () => {
    const status = await getCoreUpdaterStatus()
    if (!status.service_mode && getSystem() === 'windows') {
      showNotice.info('settings.modals.clashCore.upgradeHint')
      return null
    }
    try {
      await upgradeCore()
    } catch (err) {
      if (String(err).includes('already using latest version')) {
        return false
      }
      throw err
    }
    await repinCoreBinaries()
    return true
  }

  const onUpgrade = useLockFn(async () => {
    try {
      setUpgrading(true)
      const updated = await upgradeThroughCore()
      setUpgrading(false)
      if (updated === null) return
      mutateVersion()
      if (!updated) {
        showNotice.info(
          'settings.feedback.notifications.clash.alreadyLatestVersion',
        )
        return
      }
      showNotice.success('settings.feedback.notifications.clash.versionUpdated')
    } catch (err) {
      setUpgrading(false)
      showNotice.error(
        'settings.feedback.notifications.clash.upgradeFailed',
        err,
      )
    }
  })

  return (
    <BaseDialog
      open={open}
      title={
        <Box sx={{ display: 'flex', justifyContent: 'space-between' }}>
          {t('settings.sections.clash.form.fields.clashCore')}
          <Box>
            <Button
              variant="contained"
              size="small"
              startIcon={<SwitchAccessShortcutRounded />}
              loadingPosition="start"
              loading={upgrading}
              disabled={restarting || changingCore !== null}
              sx={{ marginRight: '8px' }}
              onClick={onUpgrade}
            >
              {t('shared.actions.upgrade')}
            </Button>
            <Button
              variant="contained"
              size="small"
              startIcon={<RestartAltRounded />}
              loadingPosition="start"
              loading={restarting}
              disabled={upgrading}
              onClick={onRestart}
            >
              {t('shared.actions.restart')}
            </Button>
          </Box>
        </Box>
      }
      contentSx={{
        pb: 0,
        width: 400,
        height: 240,
        overflowY: 'auto',
        userSelect: 'text',
        marginTop: '-8px',
      }}
      disableOk
      cancelBtn={t('shared.actions.close')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
    >
      <List component="nav">
        {VALID_CORE.map((each) => (
          <ListItemButton
            key={each.core}
            selected={each.core === clash_core}
            onClick={() => onCoreChange(each.core)}
            disabled={changingCore !== null || restarting || upgrading}
          >
            <ListItemText primary={each.name} />
            {changingCore === each.core ? (
              <CircularProgress size={20} sx={{ mr: 1 }} />
            ) : (
              <Chip label={t(each.chipKey)} size="small" />
            )}
          </ListItemButton>
        ))}
      </List>
      <Typography variant="caption" color="text.secondary" component="p">
        {verge?.use_managed_core
          ? t('settings.modals.clashCore.managedActiveNote')
          : t('settings.modals.clashCore.upgradeHint')}
      </Typography>
    </BaseDialog>
  )
}
