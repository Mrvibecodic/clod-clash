import { Box, Button } from '@mui/material'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useLockFn } from 'ahooks'
import type { ReactNode, Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog, DialogRef, FormSection } from '@/components/base'
import { createLocalBackup, importLocalBackup } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

import { AutoBackupSettings } from './auto-backup-settings'
import { BackupHistoryViewer } from './backup-history-viewer'

const ManualSection = ({
  title,
  description,
  primary,
  secondary,
  link,
}: {
  title: string
  description: string
  primary: ReactNode
  secondary: ReactNode
  link: ReactNode
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
  </>
)

export function BackupViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [backingUp, setBackingUp] = useState(false)
  const [localImporting, setLocalImporting] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [historyPage, setHistoryPage] = useState(0)

  useImperativeHandle(ref, () => ({
    open: () => setOpen(true),
    close: () => setOpen(false),
  }))

  const openHistory = () => {
    setHistoryPage(0)
    setHistoryOpen(true)
  }

  const handleBackup = useLockFn(async () => {
    try {
      setBackingUp(true)
      await createLocalBackup()
      showNotice.success('settings.modals.backup.messages.localBackupCreated')
    } catch (error) {
      console.error(error)
      showNotice.error('settings.modals.backup.messages.localBackupFailed')
    } finally {
      setBackingUp(false)
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
      openHistory()
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

  const isLocalBusy = backingUp || localImporting

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
            loading={backingUp}
            disabled={localImporting}
            onClick={() => handleBackup()}
          >
            {t('settings.modals.backup.actions.backup')}
          </Button>
        }
        secondary={
          <Button
            variant="outlined"
            size="small"
            disabled={isLocalBusy}
            onClick={() => openHistory()}
          >
            {t('settings.modals.backup.actions.viewHistory')}
          </Button>
        }
        link={
          <Button
            variant="text"
            size="small"
            loading={localImporting}
            disabled={backingUp}
            onClick={() => handleImport()}
          >
            {t('settings.modals.backup.actions.importBackup')}
          </Button>
        }
      />

      <BackupHistoryViewer
        open={historyOpen}
        page={historyPage}
        onPageChange={setHistoryPage}
        onClose={() => setHistoryOpen(false)}
      />
    </BaseDialog>
  )
}
