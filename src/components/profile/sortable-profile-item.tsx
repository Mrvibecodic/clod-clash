import { useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { Box } from '@mui/material'

import { ProfileItem, type ProfileItemProps } from './profile-item'

type SortableProfileItemProps = Omit<
  ProfileItemProps,
  'dragHandleRef' | 'dragHandleAttributes' | 'dragHandleListeners'
> & {
  id: string
}

const SORT_TRANSITION = 'transform 160ms cubic-bezier(0.2, 0, 0, 1)'

export const SortableProfileItem = ({
  id,
  ...profileItemProps
}: SortableProfileItemProps) => {
  const {
    attributes,
    listeners,
    setNodeRef,
    setActivatorNodeRef,
    transform,
    isDragging,
    isSorting,
  } = useSortable({ id, transition: null })

  const isDiagonalMove =
    transform !== null && transform.x !== 0 && transform.y !== 0

  return (
    <Box
      ref={setNodeRef}
      sx={{
        position: 'relative',
        transform: CSS.Translate.toString(transform),
        transition:
          isSorting && !isDragging && !isDiagonalMove
            ? SORT_TRANSITION
            : undefined,
        zIndex: isDragging ? 'calc(infinity)' : undefined,
      }}
    >
      <ProfileItem
        {...profileItemProps}
        dragHandleRef={setActivatorNodeRef}
        dragHandleAttributes={attributes}
        dragHandleListeners={listeners}
      />
    </Box>
  )
}
