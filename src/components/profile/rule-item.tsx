import { useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import {
  ArrowForwardRounded,
  DeleteRounded,
  DragIndicatorRounded,
  UndoRounded,
} from '@mui/icons-material'
import { alpha, Box, IconButton, Tooltip } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { policyName, ruleTypeName } from '@/utils/rule-labels'

interface Props {
  type: 'prepend' | 'original' | 'delete' | 'append'
  ruleRaw: string
  onDelete: () => void
}

export const RuleItem = (props: Props) => {
  const { type, ruleRaw, onDelete } = props
  const { t } = useTranslation()
  const sortable = type === 'prepend' || type === 'append'
  const noResolve = ruleRaw.endsWith(',no-resolve')
  const rule = ruleRaw.replace(',no-resolve', '')

  const ruleType = rule.match(/^[^,]+/)?.[0] ?? ''
  const proxyPolicy = rule.match(/[^,]+$/)?.[0] ?? ''
  const ruleContent = rule.slice(ruleType.length + 1, -proxyPolicy.length - 1)

  const $sortable = useSortable({ id: ruleRaw })

  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = sortable
    ? $sortable
    : {
        attributes: {},
        listeners: {},
        setNodeRef: undefined,
        transform: null,
        transition: undefined,
        isDragging: false,
      }

  const deleted = type === 'delete'
  const policyColor =
    proxyPolicy === 'DIRECT'
      ? 'success.main'
      : proxyPolicy.startsWith('REJECT')
        ? 'error.main'
        : 'text.primary'
  const actionLabel = deleted
    ? t('rules.modals.editor.list.actions.restore')
    : sortable
      ? t('rules.modals.editor.list.actions.remove')
      : t('rules.modals.editor.list.actions.delete')

  return (
    <Box
      ref={setNodeRef}
      sx={({ palette }) => ({
        display: 'grid',
        gridTemplateColumns: '18px 136px minmax(0, 1fr) auto 34px',
        alignItems: 'center',
        columnGap: 1.25,
        minHeight: 44,
        my: 0.5,
        pl: 1,
        pr: 0.75,
        borderRadius: '10px',
        bgcolor: deleted
          ? alpha(palette.error.main, 0.12)
          : alpha(palette.text.primary, sortable ? 0.07 : 0.04),
        boxShadow: sortable
          ? `inset 3px 0 0 ${palette.success.main}`
          : undefined,
        transform: CSS.Transform.toString(transform),
        transition,
        zIndex: isDragging ? 1 : undefined,
        position: 'relative',
      })}
    >
      {sortable ? (
        <Box
          {...attributes}
          {...listeners}
          sx={{ display: 'flex', cursor: 'grab', color: 'text.disabled' }}
        >
          <DragIndicatorRounded sx={{ fontSize: 18 }} />
        </Box>
      ) : (
        <span />
      )}

      <Box
        component="span"
        title={ruleTypeName(t, ruleType)}
        sx={({ palette }) => ({
          justifySelf: 'start',
          maxWidth: '100%',
          px: 0.75,
          py: 0.375,
          borderRadius: '5px',
          bgcolor: alpha(palette.primary.main, 0.14),
          color: 'primary.main',
          fontFamily: 'monospace',
          fontSize: 11,
          fontWeight: 600,
          lineHeight: 1.3,
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
        })}
      >
        {ruleType}
      </Box>

      <Box
        sx={{
          minWidth: 0,
          display: 'flex',
          alignItems: 'center',
          gap: 1,
        }}
      >
        <Box
          component="span"
          title={ruleContent || '-'}
          sx={{
            minWidth: 0,
            fontFamily: 'monospace',
            fontSize: 13.5,
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            textDecoration: deleted ? 'line-through' : undefined,
            color: deleted ? 'text.secondary' : 'text.primary',
          }}
        >
          {ruleContent || '—'}
        </Box>
        {deleted ? (
          <Box
            component="span"
            sx={({ palette }) => ({
              flex: 'none',
              px: 0.75,
              borderRadius: '5px',
              fontSize: 11,
              fontWeight: 600,
              color: 'error.main',
              bgcolor: alpha(palette.error.main, 0.14),
            })}
          >
            {t('rules.modals.editor.list.willDelete')}
          </Box>
        ) : null}
      </Box>

      <Box
        title={proxyPolicy}
        sx={{
          justifySelf: 'end',
          maxWidth: 200,
          display: 'flex',
          alignItems: 'center',
          gap: 0.75,
          fontSize: 13,
          color: 'text.secondary',
          whiteSpace: 'nowrap',
          overflow: 'hidden',
        }}
      >
        <ArrowForwardRounded sx={{ fontSize: 14, color: 'text.disabled' }} />
        <Box
          component="span"
          sx={{
            fontWeight: 600,
            color: policyColor,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
          }}
        >
          {policyName(t, proxyPolicy)}
        </Box>
        {noResolve ? (
          <Box component="span" sx={{ color: 'text.disabled', fontSize: 11.5 }}>
            · no-resolve
          </Box>
        ) : null}
      </Box>

      <Tooltip title={actionLabel}>
        <IconButton
          size="small"
          aria-label={actionLabel}
          onClick={onDelete}
          sx={{ color: deleted ? 'error.main' : 'text.secondary' }}
        >
          {deleted ? (
            <UndoRounded fontSize="small" />
          ) : (
            <DeleteRounded fontSize="small" />
          )}
        </IconButton>
      </Tooltip>
    </Box>
  )
}
