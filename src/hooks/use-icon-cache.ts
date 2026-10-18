import { convertFileSrc } from '@tauri-apps/api/core'
import { useMemo } from 'react'

import { downloadIconCache } from '@/services/cmds'
import { useQuery } from '@/services/query-client'

export interface UseIconCacheOptions {
  icon?: string | null
  cacheKey?: string
}

const getFileNameFromUrl = (url: string) => {
  const lastSlashIndex = url.lastIndexOf('/')
  return lastSlashIndex >= 0 ? url.slice(lastSlashIndex + 1) : url
}

export const useIconCache = ({ icon, cacheKey }: UseIconCacheOptions) => {
  const iconValue = icon?.trim() ?? ''
  const cacheKeyValue = cacheKey?.trim() ?? ''

  const isEnabled = useMemo(() => {
    return iconValue.startsWith('http') && cacheKeyValue !== ''
  }, [iconValue, cacheKeyValue])

  const { data } = useQuery({
    queryKey: ['icon-cache', iconValue, cacheKeyValue],
    queryFn: async () => {
      const fileName = `${cacheKeyValue}-${getFileNameFromUrl(iconValue)}`
      const iconPath = await downloadIconCache(iconValue, fileName)
      return convertFileSrc(iconPath)
    },
    enabled: isEnabled,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    staleTime: Infinity,
    retry: 2,
  })

  if (!isEnabled) {
    return ''
  }

  return data ?? ''
}
