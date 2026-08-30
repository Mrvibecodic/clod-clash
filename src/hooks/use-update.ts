import { fetchCacheData, setCacheData, useQuery } from '@/services/query-client'
import { checkUpdateSafe } from '@/services/update'

import { useVerge } from './use-verge'

const LAST_CHECK_KEY = 'last_check_update'

const readLastCheckTime = (): number | null => {
  try {
    const stored = localStorage.getItem(LAST_CHECK_KEY)
    if (!stored) return null
    const ts = parseInt(stored, 10)
    return Number.isNaN(ts) ? null : ts
  } catch {
    return null
  }
}

const storeLastCheckTime = (value: number) => {
  try {
    localStorage.setItem(LAST_CHECK_KEY, value.toString())
    return true
  } catch {
    return false
  }
}

export const updateLastCheckTime = (timestamp?: number): number => {
  const now = timestamp ?? Date.now()
  storeLastCheckTime(now)
  setCacheData([LAST_CHECK_KEY], now)
  return now
}

export const useUpdate = (enabled: boolean = true) => {
  const { verge } = useVerge()
  const { auto_check_update } = verge || {}

  const shouldCheck = enabled && auto_check_update !== false

  const fetchUpdate = async () => {
    const result = await checkUpdateSafe()
    updateLastCheckTime()
    return result
  }

  const { data: updateInfo, isFetching: isValidating } = useQuery({
    queryKey: ['checkUpdate'],
    queryFn: fetchUpdate,
    enabled: shouldCheck,
    retry: 2,
    staleTime: 10 * 60 * 1000,
    refetchInterval: 24 * 60 * 60 * 1000,
    refetchIntervalInBackground: true,
    refetchOnWindowFocus: false,
  })

  const checkUpdate = async () => {
    const data = await fetchCacheData(['checkUpdate'], fetchUpdate)
    return { data }
  }

  const { data: lastCheckUpdate } = useQuery({
    queryKey: [LAST_CHECK_KEY],
    queryFn: readLastCheckTime,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })

  return {
    updateInfo,
    checkUpdate,
    loading: isValidating,
    lastCheckUpdate: lastCheckUpdate ?? null,
  }
}
