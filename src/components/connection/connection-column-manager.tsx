import {
  closestCenter,
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import { arrayMove, SortableContext, useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { DragIndicatorRounded, UndoRounded } from '@mui/icons-material'
import { alpha, Box, Button, Checkbox } from '@mui/material'
import { useCallback, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseDialog } from '@/components/base'

export interface ConnectionColumnOption {
  id: string
  label: string
  visible: boolean
  toggleVisibility: (visible: boolean) => void
}

interface Props {
  open: boolean
  columns: ConnectionColumnOption[]
  onClose: () => void
  onOrderChange: (order: string[]) => void
  onReset: () => void
}

export const ConnectionColumnManager = ({
  open,
  columns,
  onClose,
  onOrderChange,
  onReset,
}: Props) => {
  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: { distance: 6 },
    }),
  )
  const { t } = useTranslation()

  const items = useMemo(() => columns.map((column) => column.id), [columns])
  const visibleCount = useMemo(
    () => columns.filter((column) => column.visible).length,
    [columns],
  )

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event
      if (!over || active.id === over.id) return

      const order = columns.map((column) => column.id)
      const oldIndex = order.indexOf(active.id as string)
      const newIndex = order.indexOf(over.id as string)
      if (oldIndex === -1 || newIndex === -1) return

      onOrderChange(arrayMove(order, oldIndex, newIndex))
    },
    [columns, onOrderChange],
  )

  return (
    <BaseDialog
      open={open}
      title={t('connections.components.columnManager.title')}
      titleExtra={
        <Box
          component="span"
          sx={{ fontSize: 12.5, fontWeight: 400, color: 'text.secondary' }}
        >
          {t('connections.components.columnManager.shown', {
            shown: visibleCount,
            total: columns.length,
          })}
        </Box>
      }
      dividers
      fullWidth
      maxWidth="xs"
      contentSx={{ py: 1 }}
      footerStart={
        <Button
          size="small"
          startIcon={<UndoRounded />}
          onClick={onReset}
          sx={{ ml: -1 }}
        >
          {t('shared.actions.resetToDefault')}
        </Button>
      }
      okBtn={t('shared.actions.close')}
      disableCancel
      onOk={onClose}
      onClose={onClose}
    >
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragEnd={handleDragEnd}
      >
        <SortableContext items={items}>
          {columns.map((column) => (
            <SortableColumnItem
              key={column.id}
              column={column}
              dragHandleLabel={t(
                'connections.components.columnManager.dragHandle',
              )}
              disableToggle={column.visible && visibleCount <= 1}
            />
          ))}
        </SortableContext>
      </DndContext>
    </BaseDialog>
  )
}

interface SortableColumnItemProps {
  column: ConnectionColumnOption
  dragHandleLabel: string
  disableToggle?: boolean
}

const SortableColumnItem = ({
  column,
  dragHandleLabel,
  disableToggle = false,
}: SortableColumnItemProps) => {
  const {
    attributes,
    listeners,
    setNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({ id: column.id })

  return (
    <Box
      ref={setNodeRef}
      sx={({ palette }) => ({
        display: 'flex',
        alignItems: 'center',
        gap: 0.5,
        minHeight: 40,
        my: 0.75,
        pl: 0.5,
        pr: 1.5,
        borderRadius: '10px',
        bgcolor: alpha(palette.text.primary, column.visible ? 0.07 : 0.035),
        position: 'relative',
        zIndex: isDragging ? 1 : undefined,
      })}
      style={{ transform: CSS.Transform.toString(transform), transition }}
    >
      <Box
        component="span"
        aria-label={dragHandleLabel}
        {...attributes}
        {...listeners}
        sx={{
          display: 'flex',
          p: 0.5,
          color: 'text.disabled',
          cursor: isDragging ? 'grabbing' : 'grab',
          touchAction: 'none',
        }}
      >
        <DragIndicatorRounded sx={{ fontSize: 18 }} />
      </Box>
      <Checkbox
        size="small"
        checked={column.visible}
        disabled={disableToggle}
        onChange={(event) => column.toggleVisibility(event.target.checked)}
        sx={{ p: 0.5 }}
      />
      <Box
        component="span"
        sx={{
          ml: 0.5,
          fontSize: 14,
          color: column.visible ? 'text.primary' : 'text.secondary',
          transition: 'color 150ms',
        }}
      >
        {column.label}
      </Box>
    </Box>
  )
}
