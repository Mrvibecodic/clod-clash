import DeleteOutlined from '@mui/icons-material/DeleteOutlined'
import DownloadRounded from '@mui/icons-material/DownloadRounded'
import RefreshRounded from '@mui/icons-material/RefreshRounded'
import RestoreRounded from '@mui/icons-material/RestoreRounded'
import { Box, Button, IconButton, Stack, Typography } from '@mui/material'
import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import customParseFormat from 'dayjs/plugin/customParseFormat'
import relativeTime from 'dayjs/plugin/relativeTime'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  BaseLoadingOverlay,
  FormHint,
  FormTile,
} from '@/components/base'
import { MONO_TEXT } from '@/components/base/base-mono'
import {
  deleteLocalBackup,
  exportLocalBackup,
  listLocalBackup,
  restartApp,
  restoreLocalBackup,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { backupHistoryNote } from '@/utils/backup-history'
import { dateTimeText } from '@/utils/date-text'

dayjs.extend(customParseFormat)
dayjs.extend(relativeTime)

const DATE_FORMAT = 'YYYY-MM-DD_HH-mm-ss'
const FILENAME_PATTERN = /\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}/

type PendingConfirmation = {
  action: 'delete' | 'restore'
  filename: string
} | null

interface BackupHistoryViewerProps {
  open: boolean
  page: number
  onPageChange: (page: number) => void
  onClose: () => void
}

interface BackupRow {
  filename: string
  platform: string
  backup_time: dayjs.Dayjs | null
  display_time: string
  sort_value: number
}

export const BackupHistoryViewer = ({
  open,
  page,
  onPageChange,
  onClose,
}: BackupHistoryViewerProps) => {
  const { t } = useTranslation()
  const [rows, setRows] = useState<BackupRow[]>([])
  const [listed, setListed] = useState(false)
  const [loading, setLoading] = useState(false)
  const [isRestoring, setIsRestoring] = useState(false)
  const [isRestarting, setIsRestarting] = useState(false)
  const [isConfirming, setIsConfirming] = useState(false)
  const [pendingConfirmation, setPendingConfirmation] =
    useState<PendingConfirmation>(null)
  const pageSize = 8
  const isBusy = loading || isRestoring || isRestarting || isConfirming

  const buildRow = useCallback(
    (item: ILocalBackupFile): BackupRow | null => {
      const { filename, last_modified } = item
      if (!filename.toLowerCase().endsWith('.zip')) return null

      const platform =
        (filename.includes('-') && filename.split('-')[0]) ||
        t('settings.modals.backup.history.unknownPlatform', {
          defaultValue: 'unknown',
        })
      const match = filename.match(FILENAME_PATTERN)
      const parsedFromName = match ? dayjs(match[0], DATE_FORMAT, true) : null
      const parsedFromModified =
        last_modified && dayjs(last_modified).isValid()
          ? dayjs(last_modified)
          : null
      const backupTime = parsedFromName?.isValid()
        ? parsedFromName
        : parsedFromModified

      return {
        filename,
        platform,
        backup_time: backupTime ?? null,
        display_time: backupTime
          ? dateTimeText(backupTime)
          : t('settings.modals.backup.history.unknownTime', {
              defaultValue: 'Unknown time',
            }),
        sort_value:
          backupTime?.valueOf() ??
          parsedFromModified?.valueOf() ??
          Number.NEGATIVE_INFINITY,
      }
    },
    [t],
  )

  const fetchRows = useCallback(async () => {
    if (!open) return
    setListed(false)

    setLoading(true)
    try {
      const list = await listLocalBackup()
      setRows(
        list
          .map((item) => buildRow(item))
          .filter((item): item is BackupRow => item !== null)
          .sort((a, b) =>
            a.sort_value === b.sort_value
              ? b.filename.localeCompare(a.filename)
              : b.sort_value - a.sort_value,
          ),
      )
      setListed(true)
    } catch (error) {
      console.error(error)
      setRows([])
      showNotice.error(error)
    } finally {
      setLoading(false)
    }
  }, [buildRow, open])

  useEffect(() => {
    void fetchRows()
  }, [fetchRows])

  const total = rows.length
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  const currentPage = Math.min(page, pageCount - 1)
  const pagedRows = rows.slice(
    currentPage * pageSize,
    currentPage * pageSize + pageSize,
  )

  const note = backupHistoryNote({ listed, total })

  const summary = useMemo(() => {
    if (note !== 'summary') return null
    const recent =
      rows[0]?.backup_time?.fromNow() ?? rows[0]?.display_time ?? ''
    return t('settings.modals.backup.history.summary', {
      count: total,
      recent,
    })
  }, [note, rows, t, total])

  const handleDelete = (filename: string) => {
    if (isRestarting) return
    setPendingConfirmation({ action: 'delete', filename })
  }

  const handleRestore = (filename: string) => {
    if (isRestoring || isRestarting) return
    setPendingConfirmation({ action: 'restore', filename })
  }

  const handleConfirmAction = useLockFn(async () => {
    if (!pendingConfirmation) return
    const { action, filename } = pendingConfirmation
    setIsConfirming(true)
    if (action === 'restore') {
      setIsRestoring(true)
    }
    try {
      if (action === 'delete') {
        await deleteLocalBackup(filename)
        setPendingConfirmation(null)
        await fetchRows()
      } else {
        await restoreLocalBackup(filename)
        setPendingConfirmation(null)
        showNotice.success('settings.modals.backup.messages.restoreSuccess')
        setIsRestarting(true)
        window.setTimeout(() => {
          void restartApp().catch((err: unknown) => {
            setIsRestarting(false)
            showNotice.error(err)
          })
        }, 1000)
      }
    } catch (error) {
      console.error(error)
      showNotice.error(error)
    } finally {
      setIsConfirming(false)
      setIsRestoring(false)
    }
  })

  const handleExport = useLockFn(async (filename: string) => {
    if (isRestarting) return
    try {
      const exported = await exportLocalBackup(filename)
      if (!exported) return
      showNotice.success('settings.modals.backup.messages.localBackupExported')
    } catch (error) {
      showNotice.error(
        'settings.modals.backup.messages.localBackupExportFailed',
        error,
      )
    }
  })

  const handleRefresh = () => {
    if (isRestarting) return
    void fetchRows()
  }

  const closeConfirmDialog = () => {
    if (isConfirming) return
    setPendingConfirmation(null)
  }

  const confirmTitle =
    pendingConfirmation?.action === 'delete'
      ? t('settings.modals.backup.actions.deleteBackup')
      : t('settings.modals.backup.actions.restoreBackup')
  const confirmMessage =
    pendingConfirmation?.action === 'delete'
      ? t('settings.modals.backup.messages.confirmDelete')
      : t('settings.modals.backup.messages.confirmRestore')

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.backup.history.title')}
      titleExtra={
        <IconButton
          size="small"
          sx={{ color: 'text.secondary' }}
          onClick={handleRefresh}
          disabled={isBusy}
        >
          <RefreshRounded fontSize="small" />
        </IconButton>
      }
      dividers
      contentSx={{ width: 532 }}
      disableOk
      cancelBtn={t('shared.actions.close')}
      onCancel={onClose}
      onClose={onClose}
    >
      <Box sx={{ position: 'relative', minHeight: 320, pt: 1.5, pb: 1 }}>
        <BaseLoadingOverlay isLoading={isBusy} />
        <Box sx={{ mb: 1, fontSize: 12.5, color: 'text.secondary' }}>
          {summary}
        </Box>

        {note === 'empty' ? (
          <FormHint>{t('settings.modals.backup.history.empty')}</FormHint>
        ) : (
          pagedRows.map((row) => (
            <FormTile key={`${row.platform}-${row.filename}`} sx={{ pr: 0.5 }}>
              <Box sx={{ flex: 1, minWidth: 0 }}>
                <Box
                  title={row.filename}
                  sx={{
                    ...MONO_TEXT,
                    fontSize: 13,
                    whiteSpace: 'nowrap',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                  }}
                >
                  {row.filename}
                </Box>
                <Box sx={{ fontSize: 12.5, color: 'text.secondary' }}>
                  {`${row.platform} · ${row.display_time}`}
                </Box>
              </Box>
              <Box sx={{ flex: 'none', display: 'flex' }}>
                <IconButton
                  size="small"
                  title={t('settings.modals.backup.actions.exportBackup')}
                  aria-label={t('settings.modals.backup.actions.exportBackup')}
                  sx={{ color: 'text.secondary' }}
                  disabled={isBusy}
                  onClick={() => handleExport(row.filename)}
                >
                  <DownloadRounded fontSize="small" />
                </IconButton>
                <IconButton
                  size="small"
                  title={t('settings.modals.backup.actions.deleteBackup')}
                  sx={{ color: 'text.secondary' }}
                  disabled={isBusy}
                  onClick={() => handleDelete(row.filename)}
                >
                  <DeleteOutlined fontSize="small" />
                </IconButton>
                <IconButton
                  size="small"
                  title={t('settings.modals.backup.actions.restoreBackup')}
                  sx={{ color: 'text.secondary' }}
                  disabled={isBusy}
                  onClick={() => handleRestore(row.filename)}
                >
                  <RestoreRounded fontSize="small" />
                </IconButton>
              </Box>
            </FormTile>
          ))
        )}

        {pageCount > 1 && (
          <Stack
            direction="row"
            spacing={1}
            sx={{ mt: 1, justifyContent: 'flex-end', alignItems: 'center' }}
          >
            <Typography
              variant="caption"
              sx={{ fontVariantNumeric: 'tabular-nums' }}
            >
              {currentPage + 1} / {pageCount}
            </Typography>
            <Button
              size="small"
              variant="text"
              disabled={isBusy || currentPage === 0}
              onClick={() => onPageChange(Math.max(0, currentPage - 1))}
            >
              {t('shared.actions.previous')}
            </Button>
            <Button
              size="small"
              variant="text"
              disabled={isBusy || currentPage >= pageCount - 1}
              onClick={() =>
                onPageChange(Math.min(pageCount - 1, currentPage + 1))
              }
            >
              {t('shared.actions.next')}
            </Button>
          </Stack>
        )}
      </Box>
      <BaseDialog
        open={pendingConfirmation !== null}
        title={confirmTitle}
        okBtn={t('shared.actions.confirm')}
        cancelBtn={t('shared.actions.cancel')}
        contentSx={{ width: { xs: 320, sm: 420 } }}
        loading={isConfirming}
        onCancel={closeConfirmDialog}
        onClose={closeConfirmDialog}
        onOk={handleConfirmAction}
      >
        <Typography variant="body2" sx={{ wordBreak: 'break-word' }}>
          {confirmMessage}
        </Typography>
        {pendingConfirmation?.filename && (
          <Typography
            variant="caption"
            color="text.secondary"
            sx={{ display: 'block', mt: 1, wordBreak: 'break-all' }}
          >
            {pendingConfirmation.filename}
          </Typography>
        )}
      </BaseDialog>
    </BaseDialog>
  )
}
