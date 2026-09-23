import { useQuery } from '@/services/query-client'
import { checkUpdateSafe } from '@/services/update'

import { useVerge } from './use-verge'

export const useUpdate = () => {
  const { verge } = useVerge()
  const { auto_check_update } = verge || {}

  const shouldCheck = auto_check_update !== false

  const { data: updateInfo } = useQuery({
    queryKey: ['checkUpdate'],
    queryFn: checkUpdateSafe,
    enabled: shouldCheck,
    retry: 2,
    staleTime: 10 * 60 * 1000,
    refetchInterval: 24 * 60 * 60 * 1000,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: false,
  })

  return { updateInfo }
}
