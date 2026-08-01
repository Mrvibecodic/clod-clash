import { useEffect, useMemo } from 'react'
import useSWR from 'swr'

import { useVerge } from '@/hooks/use-verge'
import { getRuntimeConfig } from '@/services/cmds'
import delayManager from '@/services/delay'

/** Used when neither the template nor the user named a URL. */
const FALLBACK_TEST_URL = 'http://cp.cloudflare.com/generate_204'

/**
 * clod: the URL each group should actually be tested against.
 *
 * Templates in the wild give every service group its own `url:` — the YouTube
 * group is checked against YouTube, the Telegram one against Telegram. Testing
 * all of them against one generic endpoint answers a question nobody asked:
 * "is this node alive at all", not "does this group do its job".
 *
 * Priority: the group's own `url` from the running config → the user's
 * "latency test URL" setting → a neutral generate_204 endpoint.
 */
export const useGroupTestUrls = () => {
  const { verge } = useVerge()
  const { data: runtime } = useSWR('getRuntimeConfig', getRuntimeConfig, {
    revalidateOnFocus: false,
  })

  const resolved = useMemo(() => {
    const fallback =
      verge?.default_latency_test?.trim() || FALLBACK_TEST_URL
    const byGroup = new Map<string, string>()

    const groups = (runtime as { 'proxy-groups'?: unknown })?.['proxy-groups']
    if (Array.isArray(groups)) {
      for (const group of groups) {
        const name = (group as { name?: unknown })?.name
        const url = (group as { url?: unknown })?.url
        if (typeof name === 'string' && typeof url === 'string' && url.trim()) {
          byGroup.set(name, url.trim())
        }
      }
    }

    return {
      byGroup,
      urlFor: (group?: string) =>
        (group ? byGroup.get(group) : undefined) ?? fallback,
    }
  }, [runtime, verge?.default_latency_test])

  // Delays measured in a previous session live under the URL they were taken
  // with, so the list needs to know that URL before anything is tested again.
  useEffect(() => {
    resolved.byGroup.forEach((url, group) => delayManager.setUrl(group, url))
  }, [resolved])

  return resolved
}
