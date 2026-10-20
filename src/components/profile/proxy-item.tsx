import { useSortable } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { DeleteForeverRounded, UndoRounded } from '@mui/icons-material'
import {
  Box,
  IconButton,
  ListItem,
  ListItemText,
  Tooltip,
  alpha,
  styled,
} from '@mui/material'
import { useTranslation } from 'react-i18next'

import { seqActionLabelKey } from '@/components/profile/seq-action-label'

interface Props {
  type: 'prepend' | 'original' | 'delete' | 'append'
  proxy: IProxyConfig
  onDelete: () => void
}

export const ProxyItem = (props: Props) => {
  const { type, proxy, onDelete } = props
  const { t } = useTranslation()
  const sortable = type === 'prepend' || type === 'append'
  const actionLabel = t(seqActionLabelKey(type === 'delete', sortable))

  const {
    attributes: sortableAttributes,
    listeners: sortableListeners,
    setNodeRef: sortableSetNodeRef,
    transform,
    transition,
    isDragging,
  } = useSortable({
    id: proxy.name,
    disabled: !sortable,
  })
  const dragAttributes = sortable ? sortableAttributes : undefined
  const dragListeners = sortable ? sortableListeners : undefined
  const dragNodeRef = sortable ? sortableSetNodeRef : undefined

  return (
    <ListItem
      dense
      sx={({ palette }) => ({
        position: 'relative',
        background:
          type === 'original'
            ? palette.mode === 'dark'
              ? alpha(palette.background.paper, 0.3)
              : alpha(palette.grey[400], 0.3)
            : type === 'delete'
              ? alpha(palette.error.main, 0.3)
              : alpha(palette.success.main, 0.3),
        height: '100%',
        margin: '8px 0',
        borderRadius: '8px',
        transform: CSS.Transform.toString(transform),
        transition,
        zIndex: isDragging ? 'calc(infinity)' : undefined,
      })}
    >
      <ListItemText
        {...(dragAttributes ?? {})}
        {...(dragListeners ?? {})}
        ref={dragNodeRef}
        sx={{ cursor: sortable ? 'move' : '' }}
        primary={
          <StyledPrimary
            title={proxy.name}
            sx={{ textDecoration: type === 'delete' ? 'line-through' : '' }}
          >
            {proxy.name}
          </StyledPrimary>
        }
        secondary={
          <ListItemTextChild
            sx={{
              overflow: 'hidden',
              display: 'flex',
              alignItems: 'center',
              pt: '2px',
            }}
          >
            <Box sx={{ marginTop: '2px' }}>
              <StyledTypeBox>{proxy.type}</StyledTypeBox>
            </Box>
          </ListItemTextChild>
        }
        slotProps={{
          secondary: {
            sx: {
              display: 'flex',
              alignItems: 'center',
              color: '#ccc',
            },
          },
        }}
      />
      <Tooltip title={actionLabel}>
        <IconButton aria-label={actionLabel} onClick={onDelete}>
          {type === 'delete' ? <UndoRounded /> : <DeleteForeverRounded />}
        </IconButton>
      </Tooltip>
    </ListItem>
  )
}

const StyledPrimary = styled('div')`
  font-size: 15px;
  font-weight: 700;
  line-height: 1.5;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
`

const ListItemTextChild = styled('span')`
  display: block;
`

const StyledTypeBox = styled(ListItemTextChild)(({ theme }) => ({
  display: 'inline-block',
  border: '1px solid #ccc',
  borderColor: alpha(theme.palette.primary.main, 0.5),
  color: alpha(theme.palette.primary.main, 0.8),
  borderRadius: 4,
  fontSize: 10,
  padding: '0 4px',
  lineHeight: 1.5,
  marginRight: '8px',
}))
