import { ContentCopy } from '@mui/icons-material'
import {
  Alert,
  Box,
  CircularProgress,
  IconButton,
  InputAdornment,
  Snackbar,
  TextField,
  Tooltip,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useImperativeHandle, useState, type Ref } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  DialogRef,
  FormField,
  FormRow,
  Switch,
} from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { useChangeCount } from '@/hooks/use-change-count'
import { useClashInfo } from '@/hooks/use-clash'
import { useVerge } from '@/hooks/use-verge'
import { showNotice } from '@/services/notice-service'

export function ControllerViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [copiedType, setCopiedType] = useState('')
  const [copiedOpen, setCopiedOpen] = useState(false)
  const [isSaving, setIsSaving] = useState(false)

  const { clashInfo, patchInfo } = useClashInfo()
  const { verge, patchVerge } = useVerge()
  const [controller, setController] = useState(clashInfo?.server || '')
  const [secret, setSecret] = useState(clashInfo?.secret || '')
  const [enableController, setEnableController] = useState(
    verge?.enable_external_controller ?? false,
  )

  const [initial, setInitial] = useState({
    controller,
    secret,
    enableController,
  })
  const changes = useChangeCount(initial, {
    controller,
    secret,
    enableController,
  })

  // Инициализация конфига при открытии диалога
  useImperativeHandle(ref, () => ({
    open: async () => {
      const opened = {
        controller: clashInfo?.server || '',
        secret: clashInfo?.secret || '',
        enableController: verge?.enable_external_controller ?? false,
      }
      setOpen(true)
      setController(opened.controller)
      setSecret(opened.secret)
      setEnableController(opened.enableController)
      setInitial(opened)
    },
    close: () => setOpen(false),
  }))

  const onSave = useLockFn(async () => {
    if (enableController && !controller.trim()) {
      showNotice.error(
        'settings.sections.externalController.messages.addressRequired',
      )
      return
    }
    if (enableController && !secret.trim()) {
      showNotice.error(
        'settings.sections.externalController.messages.secretRequired',
      )
      return
    }

    const wasEnabled = verge?.enable_external_controller ?? false
    try {
      setIsSaving(true)
      // Адрес и секрет — раньше включения: выключенный контроллер ядро не
      // перезапускает, и включение поднимет его уже с ними, одним перезапуском.
      if (enableController) {
        await patchInfo({
          ...(controller !== clashInfo?.server && {
            'external-controller': controller,
          }),
          ...(secret !== clashInfo?.secret && { secret }),
        })
      }
      if (enableController !== wasEnabled) {
        await patchVerge({ enable_external_controller: enableController })
      }

      showNotice.success('shared.feedback.notifications.common.saveSuccess')
      setOpen(false)
    } catch (err) {
      showNotice.error(
        'shared.feedback.notifications.common.saveFailed',
        err,
        4000,
      )
    } finally {
      setIsSaving(false)
    }
  })

  // Скопировать в буфер обмена
  const handleCopyToClipboard = useLockFn(
    async (text: string, type: string) => {
      try {
        await navigator.clipboard.writeText(text)
        setCopiedType(type)
        setCopiedOpen(true)
      } catch (err) {
        console.warn('[ControllerViewer] copy to clipboard failed:', err)
        showNotice.error(
          'settings.sections.externalController.messages.copyFailed',
        )
      }
    },
  )

  const field = (
    label: string,
    value: string,
    placeholder: string,
    onChange: (value: string) => void,
    type: 'controller' | 'secret',
  ) => (
    <FormField label={label}>
      <TextField
        size="small"
        fullWidth
        sx={({ typography }) => ({
          opacity: enableController ? 1 : 0.5,
          pointerEvents: enableController ? 'auto' : 'none',
          transition: 'opacity 150ms',
          ...MONO_INPUT,
          '& input::placeholder': { fontFamily: typography.fontFamily },
        })}
        value={value}
        placeholder={placeholder}
        onChange={(e) => onChange(e.target.value)}
        disabled={isSaving || !enableController}
        slotProps={{
          input: {
            endAdornment: (
              <InputAdornment position="end">
                <Tooltip
                  title={t(
                    'settings.sections.externalController.tooltips.copy',
                  )}
                >
                  <span>
                    <IconButton
                      size="small"
                      edge="end"
                      onClick={() => handleCopyToClipboard(value, type)}
                      disabled={isSaving || !enableController}
                    >
                      <ContentCopy fontSize="small" />
                    </IconButton>
                  </span>
                </Tooltip>
              </InputAdornment>
            ),
          },
        }}
      />
    </FormField>
  )

  return (
    <BaseDialog
      open={open}
      title={t('settings.sections.externalController.title')}
      dividers
      changes={changes}
      onReset={() => {
        setController(initial.controller)
        setSecret(initial.secret)
        setEnableController(initial.enableController)
      }}
      contentSx={{ width: 432 }}
      okBtn={
        isSaving ? (
          <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
            <CircularProgress size={16} color="inherit" />
            {t('shared.statuses.saving')}
          </Box>
        ) : (
          t('shared.actions.save')
        )
      }
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <FormRow label={t('settings.sections.externalController.fields.enable')}>
        <Switch
          edge="end"
          checked={enableController}
          onChange={(e) => setEnableController(e.target.checked)}
          disabled={isSaving}
        />
      </FormRow>
      {field(
        t('settings.sections.externalController.fields.address'),
        controller,
        t('settings.sections.externalController.placeholders.address'),
        setController,
        'controller',
      )}
      {field(
        t('settings.sections.externalController.fields.secret'),
        secret,
        t('settings.sections.externalController.placeholders.secret'),
        setSecret,
        'secret',
      )}

      <Snackbar
        open={copiedOpen}
        autoHideDuration={2000}
        onClose={(_, reason) => {
          if (reason !== 'clickaway') setCopiedOpen(false)
        }}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
      >
        <Alert severity="success">
          {copiedType === 'controller'
            ? t(
                'settings.sections.externalController.messages.controllerCopied',
              )
            : t('settings.sections.externalController.messages.secretCopied')}
        </Alert>
      </Snackbar>
    </BaseDialog>
  )
}
