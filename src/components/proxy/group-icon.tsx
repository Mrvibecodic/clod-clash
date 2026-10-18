import { Box } from '@mui/material'

import { useIconCache } from '@/hooks/use-icon-cache'
import { providerImageView } from '@/utils/provider-image'

export const GroupIcon = ({ icon, name }: { icon?: string; name: string }) => {
  const cached = useIconCache({ icon, cacheKey: name.replaceAll(' ', '') })
  const view = providerImageView(icon, cached)

  if (view === undefined) return null
  if (view === 'pending') {
    return <Box sx={{ width: 32, height: 32, mr: '12px', flex: 'none' }} />
  }
  return (
    <img
      src={view.src}
      alt=""
      width="32px"
      style={{ marginRight: '12px', borderRadius: '6px' }}
    />
  )
}
