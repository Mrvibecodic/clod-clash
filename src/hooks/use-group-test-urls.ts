import { useEffect, useMemo } from 'react'

import { useRuntimeConfig } from '@/hooks/use-clash'
import { useVerge } from '@/hooks/use-verge'
import delayManager from '@/services/delay'

/** Used when neither the template nor the user named a URL. */
const FALLBACK_TEST_URL = 'http://cp.cloudflare.com/generate_204'

/** Separators for the flattened "name → url" signature. */
const PAIR_SEP = String.fromCharCode(31)
const ENTRY_SEP = String.fromCharCode(30)

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
 *
 * The running config comes from the same react-query entry the rest of the app
 * uses, so a regenerated config (profile switch, subscription update) refreshes
 * these URLs too — with a cache of its own this hook would keep testing against
 * the URLs of the previous profile.
 */
export const useGroupTestUrls = () => {
  const { verge } = useVerge()
  const { data: runtime } = useRuntimeConfig()

  // Плоская подпись «имя→url»: объект от react-query новый на каждый ответ, и
  // без этого зависящие от нас колбэки пересоздавались бы вхолостую, сбрасывая
  // отложенный автотест задержек.
  const signature = useMemo(() => {
    const groups = (runtime as { 'proxy-groups'?: unknown })?.['proxy-groups']
    if (!Array.isArray(groups)) return ''
    return groups
      .map((group) => {
        const name = (group as { name?: unknown })?.name
        const url = (group as { url?: unknown })?.url
        return typeof name === 'string' && typeof url === 'string' && url.trim()
          ? `${name}${PAIR_SEP}${url.trim()}`
          : ''
      })
      .filter(Boolean)
      .join(ENTRY_SEP)
  }, [runtime])

  const byGroup = useMemo(() => {
    const map = new Map<string, string>()
    if (!signature) return map
    for (const entry of signature.split(ENTRY_SEP)) {
      const [name, url] = entry.split(PAIR_SEP)
      if (name && url) map.set(name, url)
    }
    return map
  }, [signature])

  const fallback = verge?.default_latency_test?.trim() || FALLBACK_TEST_URL

  // Delays measured earlier live under the URL they were taken with, so the
  // list needs to know that URL before anything is tested again. Replacing the
  // whole map (instead of adding to it) matters on a profile switch: a group of
  // the same name without its own `url` must not inherit the previous one.
  useEffect(() => {
    delayManager.replaceUrls(byGroup)
  }, [byGroup])

  return useMemo(
    () => ({
      byGroup,
      urlFor: (group?: string) =>
        (group ? byGroup.get(group) : undefined) ?? fallback,
    }),
    [byGroup, fallback],
  )
}
