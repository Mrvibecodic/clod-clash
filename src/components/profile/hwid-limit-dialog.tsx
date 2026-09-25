import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
} from '@mui/material'
import { useCallback, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useTauriEvent } from '@/hooks/use-listen'
import { useVerge } from '@/hooks/use-verge'
import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

/**
 * Device identity feedback from the panel.
 *
 * Emitted by `feat::announce_device_refusal` right after a subscription whose
 * panel refused this device was added or updated: `x-hwid-max-devices-reached` /
 * `x-hwid-limit` (the device limit is full) or `x-hwid-not-supported` (the
 * panel requires an id we did not send because the user turned it off).
 */
interface HwidNotice {
  state: 'limit' | 'not_supported'
  supportUrl?: string | null
  name?: string | null
}

const EVENT_NAME = 'clod://hwid-notice'

export const HwidLimitDialog = () => {
  const { t } = useTranslation()
  const { patchVerge } = useVerge()
  const [queue, setQueue] = useState<HwidNotice[]>([])
  const notice = queue[0] ?? null

  useTauriEvent<HwidNotice>(EVENT_NAME, ({ payload }) =>
    setQueue((pending) =>
      pending.some(
        (queued) =>
          queued.name === payload.name && queued.state === payload.state,
      )
        ? pending
        : [...pending, payload],
    ),
  )

  const close = useCallback(
    () =>
      setQueue((pending) =>
        pending[0] === notice ? pending.slice(1) : pending,
      ),
    [notice],
  )

  const enableHwid = useCallback(async () => {
    try {
      await patchVerge({ enable_hwid: true })
      showNotice.success('profiles.components.hwidDialog.enabled')
      setQueue((pending) =>
        pending.filter((queued) => queued.state !== 'not_supported'),
      )
    } catch (error) {
      showNotice.error(error instanceof Error ? error.message : String(error))
    }
  }, [patchVerge])

  const openLink = useCallback(async (url?: string | null) => {
    if (!url) return
    try {
      await openWebUrl(url)
    } catch (error) {
      showNotice.error(error instanceof Error ? error.message : String(error))
    }
  }, [])

  if (!notice) return null

  const isLimit = notice.state === 'limit'
  const title = isLimit
    ? t('profiles.components.hwidDialog.limitTitle')
    : t('profiles.components.hwidDialog.requiredTitle')
  const body = isLimit
    ? t('profiles.components.hwidDialog.limitBody')
    : t('profiles.components.hwidDialog.requiredBody')

  return (
    <Dialog open onClose={close} maxWidth="xs" fullWidth>
      <DialogTitle>{title}</DialogTitle>
      <DialogContent>
        {notice.name ? (
          <DialogContentText sx={{ fontWeight: 600, mb: 1 }}>
            {notice.name}
          </DialogContentText>
        ) : null}
        <DialogContentText sx={{ whiteSpace: 'pre-line' }}>
          {body}
        </DialogContentText>
      </DialogContent>
      <DialogActions>
        <Button onClick={close}>{t('shared.actions.cancel')}</Button>
        {isLimit ? (
          notice.supportUrl ? (
            <Button
              variant="contained"
              onClick={() => void openLink(notice.supportUrl)}
            >
              {t('profiles.components.hwidDialog.support')}
            </Button>
          ) : null
        ) : (
          <Button variant="contained" onClick={enableHwid}>
            {t('profiles.components.hwidDialog.enable')}
          </Button>
        )}
      </DialogActions>
    </Dialog>
  )
}
