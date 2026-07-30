import {
  RestartAltRounded,
  SystemUpdateAltRounded,
} from '@mui/icons-material'
import {
  Box,
  Button,
  Chip,
  LinearProgress,
  MenuItem,
  Select,
  Stack,
  Typography,
} from '@mui/material'
import { listen } from '@tauri-apps/api/event'
import { useLockFn } from 'ahooks'
import type { Ref } from 'react'
import { useEffect, useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog, DialogRef, Switch } from '@/components/base'
import { useVerge } from '@/hooks/use-verge'
import {
  type CoreUpdateCheck,
  type CoreUpdaterStatus,
  checkCoreUpdate,
  disableManagedCore,
  downloadAndApplyCore,
  getCoreUpdaterStatus,
  revertCore,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

interface Progress {
  phase: string
  received: number
  total: number
}

/**
 * clod:F5 — the managed Mihomo core dialog.
 *
 * Downloading is always an explicit action; the daily auto-check only raises
 * a notice. Turning the toggle off restarts on the bundled sidecar but keeps
 * the downloaded versions for a quick switch back.
 */
export function ManagedCoreViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const { verge, patchVerge } = useVerge()

  const [open, setOpen] = useState(false)
  const [status, setStatus] = useState<CoreUpdaterStatus>()
  const [check, setCheck] = useState<CoreUpdateCheck>()
  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState<Progress>()

  const channel = verge?.managed_core_channel === 'alpha' ? 'alpha' : 'stable'
  const autoCheck = verge?.core_auto_check ?? true

  const refreshStatus = useLockFn(async () => {
    try {
      setStatus(await getCoreUpdaterStatus())
    } catch (error) {
      showNotice.error(error)
    }
  })

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)
      void refreshStatus()
    },
    close: () => setOpen(false),
  }))

  useEffect(() => {
    if (!open) return
    const unlisten = listen<Progress>('clod://core-update-progress', (event) =>
      setProgress(event.payload),
    )
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [open])

  const onCheck = useLockFn(async () => {
    setBusy(true)
    setCheck(undefined)
    try {
      setCheck(await checkCoreUpdate())
    } catch (error) {
      showNotice.error(error)
    } finally {
      setBusy(false)
    }
  })

  const onUpdate = useLockFn(async () => {
    setBusy(true)
    setProgress(undefined)
    try {
      await downloadAndApplyCore()
      showNotice.success('settings.modals.managedCore.updated')
      setCheck(undefined)
      await refreshStatus()
    } catch (error) {
      showNotice.error(error)
    } finally {
      setBusy(false)
      setProgress(undefined)
    }
  })

  const onRevert = useLockFn(async () => {
    setBusy(true)
    try {
      await revertCore()
      showNotice.success('settings.modals.managedCore.reverted')
      await refreshStatus()
    } catch (error) {
      showNotice.error(error)
    } finally {
      setBusy(false)
    }
  })

  const onToggleManaged = useLockFn(async (enabled: boolean) => {
    setBusy(true)
    try {
      if (enabled) {
        // The backend maps a `use_managed_core` change to a core restart,
        // so the toggle takes effect immediately.
        await patchVerge({ use_managed_core: true })
      } else {
        // Patches `use_managed_core: false` internally (with the restart);
        // a second patchVerge here would restart the core twice.
        await disableManagedCore()
      }
      await refreshStatus()
    } catch (error) {
      showNotice.error(error)
    } finally {
      setBusy(false)
    }
  })

  const managedOn = verge?.use_managed_core ?? false
  const progressPercent =
    progress && progress.total > 0
      ? Math.round((progress.received * 100) / progress.total)
      : undefined

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.managedCore.title')}
      contentSx={{ width: 420 }}
      okBtn={t('shared.actions.close')}
      cancelBtn={t('shared.actions.close')}
      disableFooter
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={() => setOpen(false)}
    >
      <Stack sx={{ gap: 2, py: 1 }}>
        <Stack
          direction="row"
          sx={{ alignItems: 'center', justifyContent: 'space-between' }}
        >
          <Box>
            <Typography variant="body2" sx={{ fontWeight: 600 }}>
              {t('settings.modals.managedCore.useManaged')}
            </Typography>
            <Typography variant="caption" color="text.secondary">
              {t('settings.modals.managedCore.useManagedHint')}
            </Typography>
          </Box>
          <Switch
            checked={managedOn}
            disabled={busy}
            onChange={(_, checked) => void onToggleManaged(checked)}
          />
        </Stack>

        <Stack
          direction="row"
          sx={{ alignItems: 'center', justifyContent: 'space-between' }}
        >
          <Typography variant="body2">
            {t('settings.modals.managedCore.channel')}
          </Typography>
          <Select
            size="small"
            value={channel}
            disabled={busy}
            sx={{ width: 150 }}
            onChange={(event) =>
              void patchVerge({ managed_core_channel: event.target.value })
            }
          >
            <MenuItem value="stable">
              {t('settings.modals.managedCore.stable')}
            </MenuItem>
            <MenuItem value="alpha">
              {t('settings.modals.managedCore.alpha')}
            </MenuItem>
          </Select>
        </Stack>

        <Stack
          direction="row"
          sx={{ alignItems: 'center', justifyContent: 'space-between' }}
        >
          <Typography variant="body2">
            {t('settings.modals.managedCore.autoCheck')}
          </Typography>
          <Switch
            checked={autoCheck}
            disabled={busy}
            onChange={(_, checked) =>
              void patchVerge({ core_auto_check: checked })
            }
          />
        </Stack>

        <Box sx={{ height: 1, bgcolor: 'divider' }} />

        <Stack sx={{ gap: 0.5 }}>
          <Stack direction="row" sx={{ gap: 1, alignItems: 'center' }}>
            <Typography variant="body2" color="text.secondary">
              {t('settings.modals.managedCore.running')}
            </Typography>
            <Typography variant="body2" sx={{ fontVariantNumeric: 'tabular-nums' }}>
              {status?.running ?? '—'}
            </Typography>
            {status?.managed_active ? (
              <Chip
                size="small"
                color="success"
                label={t('settings.modals.managedCore.managedTag')}
              />
            ) : (
              <Chip
                size="small"
                label={t('settings.modals.managedCore.bundledTag')}
              />
            )}
          </Stack>
          {check ? (
            <Typography variant="caption" color="text.secondary">
              {check.update_available
                ? t('settings.modals.managedCore.available', {
                    version: check.latest,
                  })
                : t('settings.modals.managedCore.upToDate')}
            </Typography>
          ) : null}
          {status?.service_mode ? (
            // Managed core is sidecar-only: the elevated service must not
            // execute a user-writable binary (privilege boundary).
            <Typography variant="caption" color="warning.main">
              {t('settings.modals.managedCore.serviceModeNote')}
            </Typography>
          ) : null}
        </Stack>

        {progress ? (
          <Box>
            <LinearProgress
              variant={
                progressPercent === undefined ? 'indeterminate' : 'determinate'
              }
              value={progressPercent}
            />
            <Typography variant="caption" color="text.secondary">
              {t(
                `settings.modals.managedCore.phase.${progress.phase}` as never,
                { defaultValue: progress.phase },
              )}
            </Typography>
          </Box>
        ) : null}

        <Stack direction="row" sx={{ gap: 1, flexWrap: 'wrap' }}>
          <Button
            size="small"
            variant="outlined"
            disabled={busy}
            onClick={() => void onCheck()}
          >
            {t('settings.modals.managedCore.check')}
          </Button>
          <Button
            size="small"
            variant="contained"
            startIcon={<SystemUpdateAltRounded />}
            disabled={busy}
            onClick={() => void onUpdate()}
          >
            {t('settings.modals.managedCore.update')}
          </Button>
          {managedOn && status?.previous ? (
            // Only while the managed core is on — a revert with the toggle
            // off would shuffle pointers and restart onto the sidecar while
            // claiming "reverted".
            <Button
              size="small"
              color="inherit"
              startIcon={<RestartAltRounded />}
              disabled={busy}
              onClick={() => void onRevert()}
            >
              {t('settings.modals.managedCore.revert', {
                version: status.previous,
              })}
            </Button>
          ) : null}
        </Stack>
      </Stack>
    </BaseDialog>
  )
}
