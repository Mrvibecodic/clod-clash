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
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { BaseDialog, DialogRef } from '@/components/base'
import { useClash, useClashInfo } from '@/hooks/use-clash'
import { useVerge } from '@/hooks/use-verge'
import {
  type CoreUpdaterStatus,
  type SelfUpgrade,
  changeClashCore,
  getCoreUpdaterStatus,
  restartCore,
  updateBundledCore,
  upgradeCoreItself,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { coreRestartsCleanly } from '@/utils/core-self-restart'
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
  const [askingToReboot, setAskingToReboot] = useState(false)
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

  // Каждое из двух ядер обновляется со своего источника: Clod Core — с
  // релизов clod-core, Mihomo — с MetaCubeX. Кто заменит файл, решает бэкенд:
  // - 'app' — папка программы доступна на запись (обычно macOS; на Linux
  //   пакет ставит ядро в системную папку): приложение скачивает ядро, сверяет
  //   sha256 и подменяет файл при остановленном ядре, а не поднялось новое —
  //   возвращает прежнее;
  // - 'core' — папка только для администратора: ядро обновляет себя само через
  //   службу (/upgrade), а бэкенд дожидается его ответа и сверяет версию. На
  //   Windows после этого стоковое ядро и прежние сборки Clod Core оставляют
  //   копию, которая держит порт до перезагрузки, — об этом спрашиваем
  //   заранее; свежий Clod Core просто выходит;
  // - 'unavailable' — обновится вместе с приложением.
  const runUpgrade = useLockFn(async (method: CoreUpdaterStatus['method']) => {
    try {
      setUpgrading(true)
      let result: SelfUpgrade
      if (method === 'app') {
        const { updated, version } = await updateBundledCore()
        result = updated
          ? { outcome: 'updated', version }
          : { outcome: 'already_latest' }
      } else {
        result = await upgradeCoreItself()
      }
      setUpgrading(false)
      mutateVersion()
      switch (result.outcome) {
        case 'updated':
          showNotice.success(
            'settings.feedback.notifications.clash.versionUpdated',
          )
          break
        case 'already_latest':
          showNotice.info(
            'settings.feedback.notifications.clash.alreadyLatestVersion',
          )
          break
        case 'still_old':
          showNotice.warning(
            'settings.feedback.notifications.clash.upgradeNotApplied',
            { version: result.version },
          )
          break
        case 'silent':
          showNotice.error(
            'settings.feedback.notifications.clash.upgradeNoAnswer',
            { seconds: result.waited_secs },
          )
          break
      }
    } catch (err) {
      setUpgrading(false)
      mutateVersion()
      showNotice.error(
        'settings.feedback.notifications.clash.upgradeFailed',
        err,
      )
    }
  })

  const onUpgrade = useLockFn(async () => {
    let method: CoreUpdaterStatus['method']
    try {
      const status = await getCoreUpdaterStatus()
      method = status.method
      if (method === 'unavailable') {
        showNotice.info('settings.modals.clashCore.upgradeHint')
        return
      }
      // Приложение обновляет только работающее ядро: проверить, поднимется ли
      // новое, на остановленном не на чем.
      if (method === 'app' && !status.core_running) {
        showNotice.info('settings.modals.clashCore.startCoreFirst')
        return
      }
      if (
        method === 'core' &&
        getSystem() === 'windows' &&
        !coreRestartsCleanly(status.running)
      ) {
        setAskingToReboot(true)
        return
      }
    } catch (err) {
      showNotice.error(
        'settings.feedback.notifications.clash.upgradeFailed',
        err,
      )
      return
    }
    await runUpgrade(method)
  })

  const upgradeAfterAsking = () => {
    setAskingToReboot(false)
    void runUpgrade('core')
  }

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
        {t('settings.modals.clashCore.upgradeHint')}
      </Typography>
      <BaseDialog
        open={askingToReboot}
        title={t('settings.modals.clashCore.rebootAfterUpgrade.title')}
        okBtn={t('shared.actions.upgrade')}
        cancelBtn={t('shared.actions.cancel')}
        contentSx={{ width: { xs: 320, sm: 420 } }}
        onOk={upgradeAfterAsking}
        onCancel={() => setAskingToReboot(false)}
        onClose={() => setAskingToReboot(false)}
      >
        <Typography variant="body2">
          {t('settings.modals.clashCore.rebootAfterUpgrade.message')}
        </Typography>
      </BaseDialog>
    </BaseDialog>
  )
}
