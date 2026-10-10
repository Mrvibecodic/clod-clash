import { Button, InputAdornment, TextField } from '@mui/material'
import { useLockFn } from 'ahooks'
import type { Ref } from 'react'
import { useImperativeHandle, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog, DialogRef, FormRow, Switch } from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { useChangeCount } from '@/hooks/use-change-count'
import { useVerge } from '@/hooks/use-verge'
import { entry_lightweight_mode } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

export function LiteModeViewer({ ref }: { ref?: Ref<DialogRef> }) {
  const { t } = useTranslation()
  const { verge, patchVerge } = useVerge()

  const [open, setOpen] = useState(false)
  const [values, setValues] = useState({
    autoEnterLiteMode: false,
    autoEnterLiteModeDelay: 10, // По умолчанию 10 минут
  })
  const [initialValues, setInitialValues] = useState(values)
  const changes = useChangeCount(initialValues, values)

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)
      const next = {
        autoEnterLiteMode: verge?.enable_auto_light_weight_mode ?? false,
        autoEnterLiteModeDelay: verge?.auto_light_weight_minutes ?? 10,
      }
      setValues(next)
      setInitialValues(next)
    },
    close: () => setOpen(false),
  }))

  const onSave = useLockFn(async () => {
    try {
      await patchVerge({
        enable_auto_light_weight_mode: values.autoEnterLiteMode,
        auto_light_weight_minutes: values.autoEnterLiteModeDelay,
      })
      setOpen(false)
    } catch (err) {
      showNotice.error(err)
    }
  })

  return (
    <BaseDialog
      open={open}
      title={t('settings.modals.liteMode.title')}
      dividers
      changes={changes}
      onReset={() => setValues(initialValues)}
      contentSx={{ width: 452 }}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <FormRow label={t('settings.modals.liteMode.actions.enterNow')}>
        <Button
          variant="outlined"
          size="small"
          onClick={async () => await entry_lightweight_mode()}
        >
          {t('shared.actions.enable')}
        </Button>
      </FormRow>

      <FormRow
        label={t('settings.modals.liteMode.toggles.autoEnter')}
        help={t('settings.modals.liteMode.tooltips.autoEnter')}
      >
        <Switch
          edge="end"
          checked={values.autoEnterLiteMode}
          onChange={(_, c) =>
            setValues((v) => ({ ...v, autoEnterLiteMode: c }))
          }
        />
      </FormRow>

      {values.autoEnterLiteMode && (
        <FormRow
          label={t('settings.modals.liteMode.fields.delay')}
          help={t('settings.modals.liteMode.messages.autoEnterHint', {
            count: values.autoEnterLiteModeDelay,
          })}
        >
          <TextField
            autoComplete="off"
            size="small"
            type="number"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck="false"
            sx={{ width: 120, ...MONO_INPUT }}
            value={values.autoEnterLiteModeDelay}
            onChange={(e) =>
              setValues((v) => ({
                ...v,
                autoEnterLiteModeDelay: parseInt(e.target.value) || 1,
              }))
            }
            slotProps={{
              input: {
                endAdornment: (
                  <InputAdornment
                    position="end"
                    sx={{ '& p': { fontSize: 12.5 } }}
                  >
                    {t('shared.units.minutes')}
                  </InputAdornment>
                ),
              },
            }}
          />
        </FormRow>
      )}
    </BaseDialog>
  )
}
