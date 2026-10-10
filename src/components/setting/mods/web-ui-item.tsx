import {
  CheckRounded,
  CloseRounded,
  DeleteRounded,
  EditRounded,
  OpenInNewRounded,
} from '@mui/icons-material'
import { Box, IconButton, TextField } from '@mui/material'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { FormTile, TypeChip } from '@/components/base'
import { MONO_INPUT, MONO_TEXT } from '@/components/base/base-mono'

const ACTION_SX = { color: 'text.secondary' } as const

interface Props {
  value?: string
  onlyEdit?: boolean
  onChange: (value?: string) => void
  onOpenUrl?: (value?: string) => void
  onDelete?: () => void
  onCancel?: () => void
}

export const WebUIItem = (props: Props) => {
  const {
    value,
    onlyEdit = false,
    onChange,
    onDelete,
    onOpenUrl,
    onCancel,
  } = props

  const [editing, setEditing] = useState(false)
  const [editValue, setEditValue] = useState(value)
  const { t } = useTranslation()

  const highlightedParts = useMemo(() => {
    const placeholderRegex = /(%host|%port|%secret)/g
    if (!value) {
      return ['NULL']
    }
    return value.split(placeholderRegex).filter((part) => part !== '')
  }, [value])

  if (editing || onlyEdit) {
    return (
      <FormTile sx={{ pl: 0.75 }}>
        <TextField
          autoComplete="new-password"
          fullWidth
          size="small"
          value={editValue}
          onChange={(e) => setEditValue(e.target.value)}
          placeholder={t(
            'settings.modals.webUI.messages.supportedPlaceholders',
          )}
          sx={MONO_INPUT}
        />
        <IconButton
          size="small"
          title={t('shared.actions.save')}
          sx={{ color: 'primary.main' }}
          onClick={() => {
            onChange(editValue)
            setEditing(false)
          }}
        >
          <CheckRounded fontSize="small" />
        </IconButton>
        <IconButton
          size="small"
          title={t('shared.actions.cancel')}
          sx={ACTION_SX}
          onClick={() => {
            onCancel?.()
            setEditing(false)
          }}
        >
          <CloseRounded fontSize="small" />
        </IconButton>
      </FormTile>
    )
  }

  const renderedParts = highlightedParts.map((part, index) => {
    const isPlaceholder =
      part === '%host' || part === '%port' || part === '%secret'
    const repeatIndex = highlightedParts
      .slice(0, index)
      .filter((prev) => prev === part).length
    const key = `${part || 'empty'}-${repeatIndex}`

    return isPlaceholder ? (
      <TypeChip key={key} sx={{ mx: 0.25 }}>
        {part}
      </TypeChip>
    ) : (
      <span key={key}>{part}</span>
    )
  })

  return (
    <FormTile>
      <Box
        title={value}
        sx={{
          flex: 1,
          minWidth: 0,
          ...MONO_TEXT,
          fontSize: 13.5,
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          color: value ? 'text.primary' : 'text.secondary',
        }}
      >
        {renderedParts}
      </Box>
      <Box sx={{ flex: 'none', display: 'flex' }}>
        <IconButton
          size="small"
          title={t('settings.modals.webUI.actions.openUrl')}
          sx={ACTION_SX}
          onClick={() => onOpenUrl?.(value)}
        >
          <OpenInNewRounded fontSize="small" />
        </IconButton>
        <IconButton
          size="small"
          title={t('shared.actions.edit')}
          sx={ACTION_SX}
          onClick={() => {
            setEditing(true)
            setEditValue(value)
          }}
        >
          <EditRounded fontSize="small" />
        </IconButton>
        <IconButton
          size="small"
          title={t('shared.actions.delete')}
          sx={ACTION_SX}
          onClick={onDelete}
        >
          <DeleteRounded fontSize="small" />
        </IconButton>
      </Box>
    </FormTile>
  )
}
