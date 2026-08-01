import useSWR from 'swr'

import { getSentinelReport } from '@/services/cmds'
import {
  noServersReason,
  type NoServersReason,
} from '@/utils/subscription-status'

/**
 * clod: shared answer to "there is nothing to connect to — why?".
 *
 * The reason comes from the subscription data (it stays truthful even when the
 * panel hands out placeholders); the report from the config generation says
 * whether the emptiness is the panel's doing at all. Without that check a
 * template that simply ships no groups would be blamed on the provider.
 */
export const useNoServersStatus = (profile?: IProfileItem) => {
  const { data: report } = useSWR(
    profile?.uid ? ['sentinelReport', profile.uid, profile.updated ?? 0] : null,
    getSentinelReport,
    { revalidateOnFocus: false },
  )

  const reason: NoServersReason = noServersReason(profile)
  // An expired or exhausted subscription explains itself; anything else needs
  // the config-side confirmation that the panel sent placeholders.
  const show =
    Boolean(profile) && (reason !== 'provider' || Boolean(report?.only_sentinels))

  return { reason, show, remarks: report?.remarks ?? [] }
}
