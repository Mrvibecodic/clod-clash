import { StarBorderRounded, StarRounded } from '@mui/icons-material'
import { IconButton } from '@mui/material'
import { useTranslation } from 'react-i18next'

import { groupType, NON_NODE_TYPES } from '@/utils/proxy-groups'

interface Props {
  proxy: IProxyItem
  favorite?: boolean
  onToggle?: (name: string) => void
}

export const ProxyFavorite = ({ proxy, favorite = false, onToggle }: Props) => {
  const { t } = useTranslation()
  if (!onToggle || NON_NODE_TYPES.has(groupType(proxy))) return null

  return (
    <IconButton
      size="small"
      aria-label={t('home.components.serverSelect.favorite')}
      sx={{ p: 0.25, color: favorite ? 'warning.main' : 'text.disabled' }}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.preventDefault()
        e.stopPropagation()
        onToggle(proxy.name)
      }}
    >
      {favorite ? (
        <StarRounded sx={{ fontSize: 16 }} />
      ) : (
        <StarBorderRounded sx={{ fontSize: 16 }} />
      )}
    </IconButton>
  )
}
