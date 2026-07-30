import {
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogContentText,
  DialogTitle,
} from '@mui/material'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useListen } from '@/hooks/use-listen'
import { useVerge } from '@/hooks/use-verge'
import { openWebUrl } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

/**
 * Device identity feedback from the panel.
 *
 * Emitted by `config::sub_headers::SubHeaders::notify_device_state` when the
 * panel answers a subscription request with `x-hwid-max-devices-reached` /
 * `x-hwid-limit` (the device limit is full) or `x-hwid-not-supported` (the
 * panel requires an id we did not send because the user turned it off).
 */
interface HwidNotice {
  state: 'limit' | 'not_supported'
  maxDevices?: number | null
  supportUrl?: string | null
  announce?: string | null
}

const EVENT_NAME = 'clod://hwid-notice'

export const HwidLimitDialog = () => {
  const { t } = useTranslation()
  const { addListener } = useListen()
  const { patchVerge } = useVerge()
  const [notice, setNotice] = useState<HwidNotice | null>(null)

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    void Promise.resolve(
      addListener(EVENT_NAME, ({ payload }) =>
        setNotice(payload as HwidNotice),
      ),
    )
      .then((result) => {
        if (typeof result !== 'function') return
        if (disposed) {
          result()
        } else {
          unlisten = result
        }
      })
      .catch((error) =>
        console.error('[HwidLimitDialog] listener registration failed:', error),
      )

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [addListener])

  const close = useCallback(() => setNotice(null), [])

  const enableHwid = useCallback(async () => {
    try {
      await patchVerge({ enable_hwid: true })
      showNotice.success('profiles.components.hwidDialog.enabled')
      setNotice(null)
    } catch (error) {
      showNotice.error(error instanceof Error ? error.message : String(error))
    }
  }, [patchVerge])

  const openSupport = useCallback(async () => {
    if (!notice?.supportUrl) return
    try {
      await openWebUrl(notice.supportUrl)
    } catch (error) {
      showNotice.error(error instanceof Error ? error.message : String(error))
    }
  }, [notice?.supportUrl])

  if (!notice) return null

  const isLimit = notice.state === 'limit'
  const title = isLimit
    ? t('profiles.components.hwidDialog.limitTitle')
    : t('profiles.components.hwidDialog.requiredTitle')
  const body = isLimit
    ? notice.maxDevices
      ? t('profiles.components.hwidDialog.limitBodyWithCount', {
          count: notice.maxDevices,
        })
      : t('profiles.components.hwidDialog.limitBody')
    : t('profiles.components.hwidDialog.requiredBody')

  return (
    <Dialog open onClose={close} maxWidth="xs" fullWidth>
      <DialogTitle>{title}</DialogTitle>
      <DialogContent>
        <DialogContentText sx={{ whiteSpace: 'pre-line' }}>
          {body}
        </DialogContentText>
        {/* Remnawave puts the provider's maxDevicesAnnounce into `announce`. */}
        {notice.announce ? (
          <DialogContentText sx={{ mt: 2, whiteSpace: 'pre-line' }}>
            {notice.announce}
          </DialogContentText>
        ) : null}
      </DialogContent>
      <DialogActions>
        <Button onClick={close}>{t('shared.actions.cancel')}</Button>
        {isLimit ? (
          notice.supportUrl ? (
            <Button variant="contained" onClick={openSupport}>
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
