import { useEffect, useRef } from 'react'
import useSWR from 'swr'

import { useProfiles } from '@/hooks/use-profiles'
import { useVerge } from '@/hooks/use-verge'
import { getProfileBackground } from '@/services/cmds'

export const useProviderTheme = () => {
  const { verge } = useVerge()
  const { current } = useProfiles()

  const enabled = verge?.theme_setting?.provider_theme !== false
  const uid = enabled && current?.theme_background ? current.uid : undefined

  const { data: background, mutate: revalidateBackground } = useSWR(
    uid ? ['profileBackground', uid] : null,
    ([, id]) => getProfileBackground(id as string),
    { revalidateOnFocus: false },
  )

  const lastUpdatedRef = useRef(current?.updated)
  useEffect(() => {
    if (lastUpdatedRef.current === current?.updated) return
    lastUpdatedRef.current = current?.updated
    void revalidateBackground()
  }, [current?.updated, revalidateBackground])

  if (!enabled) {
    return { accent: undefined, mode: undefined, background: undefined }
  }
  return {
    accent: current?.theme_accent,
    mode: current?.theme_mode,
    background: uid ? (background ?? undefined) : undefined,
  }
}
