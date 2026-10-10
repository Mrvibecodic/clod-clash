import { Box, Button } from '@mui/material'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useLockFn } from 'ahooks'
import type { ReactNode, Ref } from 'react'
import { useCallback, useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog, DialogRef, FormSection } from '@/components/base'
import { useVerge } from '@/hooks/use-verge'
import {
  createLocalBackup,
  createWebdavBackup,
  importLocalBackup,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { buildWebdavSignature, setWebdavStatus } from '@/services/webdav-status'

import { AutoBackupSettings } from './auto-backup-settings'
import { BackupHistoryViewer } from './backup-history-viewer'
import { BackupWebdavDialog } from './backup-webdav-dialog'

type BackupSource = 'local' | 'webdav'

const ManualSection = ({
  title,
  description,
  primary,
  secondary,
  link,
  note,
}: {
  title: string
  description: string
  primary: ReactNode
  secondary: ReactNode
  link: ReactNode
  note?: ReactNode
}) => (
  <>
    <FormSection title={title} />
    <Box sx={{ mb: 1.25, fontSize: 12.5, color: 'text.secondary' }}>
      {description}
    </Box>
    <Box
      sx={{
        display: 'flex',
        flexWrap: 'wrap',
        alignItems: 'center',
        gap: 1,
        pb: 1.5,
      }}
    >
      {primary}
      {secondary}
      <Box sx={{ ml: 'auto' }}>{link}</Box>
    </Box>
    {note ? (
      <Box sx={{ mt: -0.75, pb: 1.5, fontSize: 12.5, color: 'text.secondary' }}>
        {note}
      </Box>
    ) : null}
  </>
)

export function BackupViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const { verge } = useVerge()
  const [open, setOpen] = useState(false)
  const [busyAction, setBusyAction] = useState<BackupSource | null>(null)
  const [localImporting, setLocalImporting] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [historySource, setHistorySource] = useState<BackupSource>('local')
  const [historyPage, setHistoryPage] = useState(0)
  const [webdavDialogOpen, setWebdavDialogOpen] = useState(false)
  const webdavSignature = buildWebdavSignature(verge)
  const webdavReady = Boolean(
    verge?.webdav_url?.trim() &&
      verge?.webdav_username?.trim() &&
      verge?.webdav_password,
  )

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }))

  const openHistory = (target: BackupSource) => {
    setHistorySource(target)
    setHistoryPage(0)
    setHistoryOpen(true)
  }

  const handleBackup = useLockFn(async (target: BackupSource) => {
    try {
      setBusyAction(target)
      if (target === 'local') {
        await createLocalBackup()
        showNotice.success('settings.modals.backup.messages.localBackupCreated')
      } else {
        await createWebdavBackup()
        showNotice.success('settings.modals.backup.messages.backupCreated')
        setWebdavStatus(webdavSignature, 'ready')
      }
    } catch (error) {
      console.error(error)
      showNotice.error(
        target === 'local'
          ? 'settings.modals.backup.messages.localBackupFailed'
          : 'settings.modals.backup.messages.backupFailed',
        target === 'local' ? undefined : { error },
      )
      if (target === 'webdav') {
        setWebdavStatus(webdavSignature, 'failed')
      }
    } finally {
      setBusyAction(null)
    }
  })

  const handleImport = useLockFn(async () => {
    const selected = await openDialog({
      multiple: false,
      filters: [{ name: 'Backup File', extensions: ['zip'] }],
    })
    if (!selected || Array.isArray(selected)) return
    try {
      setLocalImporting(true)
      await importLocalBackup(selected)
      showNotice.success('settings.modals.backup.messages.localBackupImported')
      openHistory('local')
    } catch (error) {
      console.error(error)
      showNotice.error(
        'settings.modals.backup.messages.localBackupImportFailed',
        { error },
      )
    } finally {
      setLocalImporting(false)
    }
  })

  const setWebdavBusy = useCallback(
    (loading: boolean) => {
      setBusyAction(loading ? 'webdav' : null)
    },
    [setBusyAction],
  )

  const isLocalBusy = busyAction === 'local' || localImporting

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.backup.title')}
      dividers
      contentSx={{ width: 532 }}
      disableOk
      cancelBtn={t('shared.actions.close')}
      onCancel={() => setOpen(false)}
      onClose={() => setOpen(false)}
    >
      <FormSection title={t('settings.modals.backup.auto.title')} />
      <AutoBackupSettings />

      <ManualSection
        title={t('settings.modals.backup.tabs.local')}
        description={t('settings.modals.backup.manual.local')}
        primary={
          <Button
            variant="contained"
            size="small"
            loading={busyAction === 'local'}
            disabled={localImporting}
            onClick={() => handleBackup('local')}
          >
            {t('settings.modals.backup.actions.backup')}
          </Button>
        }
        secondary={
          <Button
            variant="outlined"
            size="small"
            disabled={isLocalBusy}
            onClick={() => openHistory('local')}
          >
            {t('settings.modals.backup.actions.viewHistory')}
          </Button>
        }
        link={
          <Button
            variant="text"
            size="small"
            loading={localImporting}
            disabled={busyAction === 'local'}
            onClick={() => handleImport()}
          >
            {t('settings.modals.backup.actions.importBackup')}
          </Button>
        }
      />

      <ManualSection
        title={t('settings.modals.backup.tabs.webdav')}
        description={t('settings.modals.backup.manual.webdav')}
        primary={
          <Button
            variant="contained"
            size="small"
            loading={busyAction === 'webdav'}
            disabled={!webdavReady}
            onClick={() => handleBackup('webdav')}
          >
            {t('settings.modals.backup.actions.backup')}
          </Button>
        }
        secondary={
          <Button
            variant="outlined"
            size="small"
            disabled={!webdavReady}
            onClick={() => openHistory('webdav')}
          >
            {t('settings.modals.backup.actions.viewHistory')}
          </Button>
        }
        link={
          <Button
            variant="text"
            size="small"
            onClick={() => setWebdavDialogOpen(true)}
          >
            {t('settings.modals.backup.manual.configureWebdav')}
          </Button>
        }
        note={
          webdavReady
            ? undefined
            : t('settings.modals.backup.manual.webdavNotConfigured')
        }
      />

      <BackupHistoryViewer
        open={historyOpen}
        source={historySource}
        page={historyPage}
        onSourceChange={setHistorySource}
        onPageChange={setHistoryPage}
        onClose={() => setHistoryOpen(false)}
      />
      <BackupWebdavDialog
        open={webdavDialogOpen}
        onClose={() => setWebdavDialogOpen(false)}
        onBackupSuccess={() => openHistory('webdav')}
        setBusy={setWebdavBusy}
      />
    </BaseDialog>
  )
}
