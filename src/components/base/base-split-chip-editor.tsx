import { AddRounded, DeleteOutlineRounded } from '@mui/icons-material'
import {
  Box,
  Button,
  FormHelperText,
  IconButton,
  TextField,
} from '@mui/material'
import type { ReactNode } from 'react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { splitList, withListDraft } from '@/utils/list-draft'

import { BaseSegmented, FormHint, FormTile } from './base-form'
import { MONO_INPUT, MONO_TEXT } from './base-mono'

type BaseSplitChipEditorMode = 'visual' | 'advanced'

interface BaseSplitChipEditorProps {
  value?: string
  onChange: (value: string) => void
  draft: string
  onDraftChange: (draft: string) => void
  disabled?: boolean
  error?: boolean
  helperText?: ReactNode
  placeholder?: string
  separator?: string
  ariaLabel?: string
  renderHeader?: (modeToggle: ReactNode) => ReactNode
}

interface BaseListTilesProps {
  items: { key: string | number; value: string }[]
  onRemove: (index: number) => void
  draft: string
  onDraftChange: (draft: string) => void
  onAdd: () => void
  placeholder?: string
  disabled?: boolean
  error?: boolean
  addLabel?: ReactNode
}

export const BaseListTiles = ({
  items,
  onRemove,
  draft,
  onDraftChange,
  onAdd,
  placeholder,
  disabled = false,
  error = false,
  addLabel,
}: BaseListTilesProps) => {
  const { t } = useTranslation()

  return (
    <>
      {items.length ? (
        items.map((item, index) => (
          <FormTile key={item.key} sx={{ minHeight: 38 }}>
            <Box
              component="span"
              title={item.value}
              sx={{
                ...MONO_TEXT,
                fontSize: 13.5,
                flex: 1,
                minWidth: 0,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                color: disabled ? 'text.secondary' : 'text.primary',
              }}
            >
              {item.value}
            </Box>
            <IconButton
              size="small"
              aria-label={t('shared.actions.delete')}
              title={t('shared.actions.delete')}
              disabled={disabled}
              onClick={() => onRemove(index)}
            >
              <DeleteOutlineRounded fontSize="small" />
            </IconButton>
          </FormTile>
        ))
      ) : (
        <FormHint sx={{ my: 0.625 }}>{t('shared.statuses.empty')}</FormHint>
      )}
      <Box sx={{ display: 'flex', gap: 1, mt: 1, alignItems: 'center' }}>
        <TextField
          disabled={disabled}
          size="small"
          fullWidth
          value={draft}
          placeholder={placeholder}
          error={error}
          autoComplete="off"
          spellCheck="false"
          sx={({ typography }) => ({
            ...MONO_INPUT,
            '& input::placeholder': { fontFamily: typography.fontFamily },
          })}
          onChange={(event) => onDraftChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              onAdd()
            }
          }}
        />
        <Button
          variant="outlined"
          startIcon={<AddRounded />}
          onClick={onAdd}
          disabled={disabled || !draft.trim()}
          sx={{ flex: 'none', whiteSpace: 'nowrap' }}
        >
          {addLabel ?? t('shared.actions.add')}
        </Button>
      </Box>
    </>
  )
}

export const BaseSplitChipEditor = ({
  value = '',
  onChange,
  draft,
  onDraftChange,
  disabled = false,
  error = false,
  helperText,
  placeholder,
  separator = ',',
  ariaLabel,
  renderHeader,
}: BaseSplitChipEditorProps) => {
  const { t } = useTranslation()
  const [mode, setMode] = useState<BaseSplitChipEditorMode>('visual')

  const values = useMemo(() => splitList(value), [value])

  const items = useMemo(() => {
    const counts = new Map<string, number>()
    return values.map((item) => {
      const nextCount = (counts.get(item) ?? 0) + 1
      counts.set(item, nextCount)
      return {
        key: `${item}-${nextCount}`,
        value: item,
      }
    })
  }, [values])

  const handleAddDraft = () => {
    if (!splitList(draft).length) {
      return
    }
    onChange(withListDraft(value, draft, separator))
    onDraftChange('')
  }

  const handleRemoveItem = (index: number) => {
    const nextValue = values.filter((_, itemIndex) => itemIndex !== index)
    onChange(nextValue.join(separator))
  }

  const modeToggle = (
    <Box role="group" aria-label={ariaLabel}>
      <BaseSegmented
        value={mode}
        options={[
          { value: 'visual', label: t('shared.editorModes.list') },
          { value: 'advanced', label: t('shared.editorModes.text') },
        ]}
        onChange={(nextMode) => {
          setMode(nextMode)
          if (nextMode === 'advanced') {
            handleAddDraft()
          }
        }}
      />
    </Box>
  )

  return (
    <>
      {renderHeader ? (
        renderHeader(modeToggle)
      ) : (
        <Box sx={{ display: 'flex', justifyContent: 'flex-end', mb: 0.5 }}>
          {modeToggle}
        </Box>
      )}
      {mode === 'visual' ? (
        <Box sx={{ pb: 0.5 }}>
          <BaseListTiles
            items={items}
            onRemove={handleRemoveItem}
            draft={draft}
            onDraftChange={onDraftChange}
            onAdd={handleAddDraft}
            placeholder={placeholder}
            disabled={disabled}
            error={error}
          />
          {helperText && (
            <FormHelperText
              error={error}
              sx={{ mx: 0, mt: 0.75, fontSize: 12.5 }}
            >
              {helperText}
            </FormHelperText>
          )}
        </Box>
      ) : (
        <TextField
          error={error}
          disabled={disabled}
          size="small"
          multiline
          rows={4}
          sx={{ width: '100%', ...MONO_INPUT }}
          value={value}
          helperText={helperText}
          spellCheck="false"
          onChange={(event) => {
            onChange(event.target.value)
          }}
        />
      )}
    </>
  )
}
