import { Box } from '@mui/material'

import { getGroupIcon } from '@/services/cmds'
import { useQuery } from '@/services/query-client'
import {
  awaitsCopy,
  PICTURE_RETRY_MS,
  providerImageView,
} from '@/utils/provider-image'

/** Значок группы подписки `uid`: копию по адресу из её конфига держит приложение. */
export const GroupIcon = ({ icon, uid }: { icon?: string; uid?: string }) => {
  const url = icon?.trim() ?? ''
  const { data } = useQuery({
    queryKey: ['groupIcon', uid, url],
    queryFn: () => getGroupIcon(uid as string, url),
    enabled: !!uid && url.startsWith('http'),
    refetchInterval: (cached) => (awaitsCopy(cached) ? PICTURE_RETRY_MS : 0),
    refetchOnWindowFocus: false,
  })
  const view = providerImageView(icon, uid ? data : 'never')

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
