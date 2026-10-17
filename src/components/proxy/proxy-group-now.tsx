import { ArrowForwardRounded } from '@mui/icons-material'
import { Typography } from '@mui/material'

import { TypeChip } from '@/components/base'

// У широкой плитки своя запись: там строка переносится вместе с именем узла.
export const GroupNow = ({
  type,
  now,
  strong,
}: {
  type?: string
  now?: string
  strong?: boolean
}) => (
  <>
    {type && <TypeChip>{type}</TypeChip>}
    {now && (
      <>
        <ArrowForwardRounded sx={{ fontSize: 14, color: 'text.disabled' }} />
        <Typography
          variant={strong ? 'body1' : 'body2'}
          component="span"
          noWrap
          title={now}
          sx={{
            color: 'text.primary',
            fontSize: 13,
            fontWeight: strong ? 600 : undefined,
            minWidth: 0,
          }}
        >
          {now}
        </Typography>
      </>
    )}
  </>
)
