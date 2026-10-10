import { ContentCopyRounded } from '@mui/icons-material'
import { Box, CircularProgress, IconButton } from '@mui/material'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import type { Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  BaseSegmented,
  DialogRef,
  FormHint,
  FormTile,
} from '@/components/base'
import { MONO_TEXT } from '@/components/base/base-mono'
import { useNetworkInterfaces } from '@/hooks/use-network'
import { showNotice } from '@/services/notice-service'

const FAMILIES = [
  { value: 'v4', label: 'IPv4' },
  { value: 'v6', label: 'IPv6' },
]

export function NetworkInterfaceViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [isV4, setIsV4] = useState(true)

  const { networkInterfaces, loading, error, mutate } = useNetworkInterfaces()

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)
      void mutate()
    },
    close: () => setOpen(false),
  }))

  const isEmpty = networkInterfaces.length === 0
  const getAddressIp = (address: IAddress) =>
    isV4 ? address.V4?.ip : address.V6?.ip

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.networkInterface.title')}
      titleExtra={
        <BaseSegmented
          value={isV4 ? 'v4' : 'v6'}
          options={FAMILIES}
          onChange={(family) => setIsV4(family === 'v4')}
          sx={{ flex: 'none' }}
        />
      }
      dividers
      contentSx={{ width: 472 }}
      disableOk
      cancelBtn={t('shared.actions.close')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
    >
      {loading && isEmpty ? (
        <Box sx={{ display: 'flex', justifyContent: 'center', py: 4 }}>
          <CircularProgress size={24} />
        </Box>
      ) : isEmpty ? (
        <FormHint sx={{ my: 0.625 }}>
          {t('shared.statuses.empty')}
          {error ? (
            <Box sx={{ mt: 0.5, fontSize: 12.5, color: 'error.main' }}>
              {String(error)}
            </Box>
          ) : null}
        </FormHint>
      ) : (
        networkInterfaces.map((item) => (
          <FormTile
            key={item.name}
            sx={{
              flexDirection: 'column',
              alignItems: 'stretch',
              gap: 0.25,
              py: 1,
              pr: 0.5,
            }}
          >
            <Box sx={{ fontSize: 14, fontWeight: 600 }}>{item.name}</Box>
            {item.addr.map((address) => {
              const ip = getAddressIp(address)
              return (
                ip && (
                  <AddressDisplay
                    key={ip}
                    label={t(
                      'settings.modals.networkInterface.fields.ipAddress',
                    )}
                    content={ip}
                  />
                )
              )
            })}
            <AddressDisplay
              label={t('settings.modals.networkInterface.fields.macAddress')}
              content={item.mac_addr ?? ''}
            />
          </FormTile>
        ))
      )}
    </BaseDialog>
  )
}

const AddressDisplay = ({
  label,
  content,
}: {
  label: string
  content: string
}) => {
  return (
    <Box
      sx={{
        display: 'flex',
        alignItems: 'center',
        gap: 1,
        minHeight: 30,
        fontSize: 12.5,
      }}
    >
      <Box sx={{ flex: 'none', color: 'text.secondary' }}>{label}</Box>
      <Box
        sx={{
          flex: 1,
          minWidth: 0,
          ...MONO_TEXT,
          fontSize: 13,
          userSelect: 'text',
          wordBreak: 'break-all',
        }}
      >
        {content}
      </Box>
      <IconButton
        size="small"
        onClick={async () => {
          try {
            await writeText(content)
            showNotice.success(
              'shared.feedback.notifications.common.copySuccess',
            )
          } catch (err) {
            showNotice.error(err)
          }
        }}
      >
        <ContentCopyRounded sx={{ fontSize: '17px' }} />
      </IconButton>
    </Box>
  )
}
