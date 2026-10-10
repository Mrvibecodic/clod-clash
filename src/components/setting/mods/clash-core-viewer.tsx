import {
  RestartAltRounded,
  SwitchAccessShortcutRounded,
} from '@mui/icons-material'
import { Box, Button, CircularProgress, Radio, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import type { Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { BaseDialog, CodeChip, DialogRef, FormTile } from '@/components/base'
import { useClash } from '@/hooks/use-clash'
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
  const { version, mutateVersion } = useClash()

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
      // Версию и настройки нового ядра перечитывает событие обновления
      // конфига: бэкенд шлёт его до возврата.
      await changeClashCore(core)
      mutateVerge()
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

  const busy = changingCore !== null || restarting || upgrading

  return (
    <BaseDialog
      open={open}
      title={t('settings.sections.clash.form.fields.clashCore')}
      titleExtra={
        <Box sx={{ display: 'flex', flexWrap: 'wrap', gap: 1 }}>
          <Button
            variant="outlined"
            size="small"
            startIcon={<SwitchAccessShortcutRounded />}
            loadingPosition="start"
            loading={upgrading}
            disabled={restarting || changingCore !== null}
            onClick={onUpgrade}
          >
            {t('shared.actions.upgrade')}
          </Button>
          <Button
            variant="outlined"
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
      }
      dividers
      contentSx={{ width: 492, userSelect: 'text' }}
      disableOk
      cancelBtn={t('shared.actions.close')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
    >
      <Box role="radiogroup" sx={{ pt: 0.5 }}>
        {VALID_CORE.map((each) => {
          const selected = each.core === clash_core
          return (
            <FormTile
              key={each.core}
              selected={selected}
              onClick={busy ? undefined : () => onCoreChange(each.core)}
              sx={{
                minHeight: 52,
                pl: 1,
                pr: 1.25,
                opacity: busy && changingCore !== each.core ? 0.6 : 1,
                transition: 'background-color 150ms, opacity 150ms',
              }}
            >
              <Radio
                size="small"
                checked={selected}
                disabled={busy}
                disableRipple
                slotProps={{ input: { 'aria-label': each.name } }}
                sx={{ p: 0.5 }}
              />
              <Box sx={{ flex: 1, minWidth: 0 }}>
                <Box sx={{ fontSize: 14, fontWeight: 600 }}>{each.name}</Box>
                {selected && version !== '-' ? (
                  <CodeChip sx={{ mt: 0.25, color: 'text.primary' }}>
                    {version}
                  </CodeChip>
                ) : null}
              </Box>
              {changingCore === each.core ? (
                <CircularProgress size={18} sx={{ mr: 0.5 }} />
              ) : (
                <CodeChip sx={{ flex: 'none' }}>{t(each.chipKey)}</CodeChip>
              )}
            </FormTile>
          )
        })}
      </Box>
      <Box
        sx={{
          mt: 1.5,
          mb: 0.5,
          fontSize: 13,
          lineHeight: 1.55,
          color: 'text.secondary',
        }}
      >
        {t('settings.modals.clashCore.upgradeHint')}
      </Box>
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
