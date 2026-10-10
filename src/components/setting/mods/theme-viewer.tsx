import { Box, TextField, useTheme } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useImperativeHandle, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  DialogRef,
  FormField,
  FormRow,
  FormSection,
  Switch,
} from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { useChangeCount } from '@/hooks/use-change-count'
import { useVerge } from '@/hooks/use-verge'
import { ACCENT_PRESETS, defaultDarkTheme, defaultTheme } from '@/pages/_theme'
import { showNotice } from '@/services/notice-service'

export function ThemeViewer(props: { ref?: React.Ref<DialogRef> }) {
  const { ref } = props
  const { t } = useTranslation()

  const [open, setOpen] = useState(false)
  const { verge, patchVerge } = useVerge()
  const { theme_setting } = verge ?? {}
  const [theme, setTheme] = useState(theme_setting || {})
  const [initialTheme, setInitialTheme] = useState(theme)
  const changes = useChangeCount(initialTheme, theme)

  useImperativeHandle(ref, () => ({
    open: () => {
      setOpen(true)
      setTheme({ ...theme_setting })
      setInitialTheme({ ...theme_setting })
    },
    close: () => setOpen(false),
  }))

  const textProps = {
    size: 'small',
    autoComplete: 'off',
    fullWidth: true,
  } as const

  const handleChange = (field: keyof typeof theme) => (e: any) => {
    setTheme((t) => ({ ...t, [field]: e.target.value }))
  }

  const onSave = useLockFn(async () => {
    try {
      await patchVerge({ theme_setting: theme })
      setOpen(false)
    } catch (err) {
      showNotice.error(err)
    }
  })

  const { palette } = useTheme()

  const dt = palette.mode === 'light' ? defaultTheme : defaultDarkTheme

  type ThemeKey = keyof typeof theme & keyof typeof defaultTheme

  const fieldDefinitions: Array<{ labelKey: string; key: ThemeKey }> = useMemo(
    () => [
      {
        labelKey: 'settings.components.verge.theme.fields.primaryColor',
        key: 'primary_color',
      },
      {
        labelKey: 'settings.components.verge.theme.fields.secondaryColor',
        key: 'secondary_color',
      },
      {
        labelKey: 'settings.components.verge.theme.fields.primaryText',
        key: 'primary_text',
      },
      {
        labelKey: 'settings.components.verge.theme.fields.secondaryText',
        key: 'secondary_text',
      },
      {
        labelKey: 'settings.components.verge.theme.fields.infoColor',
        key: 'info_color',
      },
      {
        labelKey: 'settings.components.verge.theme.fields.warningColor',
        key: 'warning_color',
      },
      {
        labelKey: 'settings.components.verge.theme.fields.errorColor',
        key: 'error_color',
      },
      {
        labelKey: 'settings.components.verge.theme.fields.successColor',
        key: 'success_color',
      },
    ],
    [],
  )

  const renderItem = (labelKey: string, key: ThemeKey) => (
    <FormField key={key} label={t(labelKey)} sx={{ py: 0.75 }}>
      <TextField
        {...textProps}
        value={theme[key] ?? ''}
        placeholder={dt[key]}
        sx={MONO_INPUT}
        onChange={handleChange(key)}
        onKeyDown={(e) => e.key === 'Enter' && onSave()}
        slotProps={{
          input: {
            startAdornment: (
              <Box
                sx={{
                  flex: 'none',
                  width: 16,
                  height: 16,
                  mr: 1,
                  borderRadius: '50%',
                  bgcolor: theme[key] || dt[key],
                  boxShadow: (t2) => `inset 0 0 0 1px ${t2.palette.divider}`,
                  transition: 'background-color 150ms',
                }}
              />
            ),
          },
        }}
      />
    </FormField>
  )

  return (
    <BaseDialog
      open={open}
      title={t('settings.components.verge.theme.title')}
      dividers
      changes={changes}
      onReset={() => setTheme(initialTheme)}
      okBtn={t('shared.actions.save')}
      cancelBtn={t('shared.actions.cancel')}
      contentSx={{ width: 512 }}
      onClose={() => setOpen(false)}
      onCancel={() => setOpen(false)}
      onOk={onSave}
    >
      <FormRow
        label={t('settings.components.verge.theme.fields.providerTheme')}
        help={t('settings.components.verge.theme.fields.providerThemeHint')}
      >
        <Switch
          edge="end"
          checked={theme.provider_theme !== false}
          onChange={(_, checked) =>
            setTheme((prev) => ({ ...prev, provider_theme: checked }))
          }
        />
      </FormRow>

      <FormSection
        title={t('settings.components.verge.theme.fields.accentPresets')}
      />
      <Box sx={{ display: 'flex', gap: 1.5, px: 0.5, pt: 0.75, pb: 1 }}>
        {ACCENT_PRESETS.map((color) => {
          const selected = (theme.primary_color ?? dt.primary_color) === color
          return (
            <Box
              key={color}
              role="button"
              aria-pressed={selected}
              title={color}
              onClick={() =>
                setTheme((prev) => ({ ...prev, primary_color: color }))
              }
              sx={(t2) => ({
                width: 24,
                height: 24,
                borderRadius: '50%',
                bgcolor: color,
                cursor: 'pointer',
                outline: '2px solid',
                outlineOffset: 2,
                outlineColor: selected
                  ? t2.palette.text.primary
                  : 'transparent',
                transition: 'transform 150ms cubic-bezier(0.2, 0, 0, 1)',
                '&:hover': { transform: 'scale(1.08)' },
                '@media (prefers-reduced-motion: reduce)': {
                  transition: 'none',
                  '&:hover': { transform: 'none' },
                },
              })}
            />
          )
        })}
      </Box>

      <FormSection
        title={t('settings.components.verge.theme.sections.colors')}
      />
      <Box
        sx={{
          display: 'grid',
          gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
          columnGap: 1.5,
        }}
      >
        {fieldDefinitions.map((field) => renderItem(field.labelKey, field.key))}
      </Box>

      <FormField label={t('settings.components.verge.theme.fields.fontFamily')}>
        <TextField
          {...textProps}
          value={theme.font_family ?? ''}
          onChange={handleChange('font_family')}
          onKeyDown={(e) => e.key === 'Enter' && onSave()}
        />
      </FormField>
    </BaseDialog>
  )
}
