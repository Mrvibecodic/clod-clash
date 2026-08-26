import useSWR from 'swr'

import { useTauriEvent } from '@/hooks/use-listen'
import { getServerDescriptions } from '@/services/cmds'

/** Stable identity for "the panel sent no descriptions" — the common case. */
const EMPTY: Record<string, string> = {}

/**
 * clod: what the provider says about each server, keyed by node name.
 *
 * The description lives in the subscription (`serverDescription` inside the
 * node), not in the core's proxy list, so it is read from the generated config
 * — and that config is rebuilt by far more than a subscription update: a
 * merge-chain edit, a setting that touches the core, a restart. Hence the same
 * event the rest of the core data listens to, rather than a profile key.
 *
 * An empty map is normal: the panel hands the field to "extended clients" only
 * and recognises them by a built-in list of user agents we are not on, so it
 * takes a rule on the provider's side (`docs/REMNAWAVE.md`) for anything to
 * arrive. Every consumer therefore has to keep working without it.
 */
export const useServerDescriptions = () => {
  const { data, mutate } = useSWR('serverDescriptions', getServerDescriptions, {
    revalidateOnFocus: false,
  })

  useTauriEvent('verge://refresh-clash-config', () => {
    void mutate()
  })

  return data ?? EMPTY
}
