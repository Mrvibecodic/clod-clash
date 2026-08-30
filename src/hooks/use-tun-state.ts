import { getTunState } from '@/services/cmds'
import { useQuery } from '@/services/query-client'

import { useVisibility } from './use-visibility'

const defaultTunState: ITunState = {
  desired: false,
  active: false,
  capable: false,
  setup_declined: false,
  needs_repair: false,
  runtime_stack: null,
  failure: null,
}

export const useTunState = () => {
  const pageVisible = useVisibility()

  const {
    data: tun = defaultTunState,
    refetch: mutateTunState,
    isLoading,
  } = useQuery({
    queryKey: ['getTunState'],
    queryFn: getTunState,
    refetchInterval: pageVisible ? 10000 : false,
    refetchOnWindowFocus: true,
    refetchOnReconnect: true,
  })

  return {
    tunDesired: tun.desired,
    tunActive: tun.active,
    tunCapable: tun.capable,
    tunSetupDeclined: tun.setup_declined,
    tunNeedsRepair: tun.needs_repair,
    tunBroken: tun.desired && !tun.active,
    tunRuntimeStack: tun.runtime_stack ?? null,
    tunFailure: tun.failure ?? null,
    mutateTunState,
    isLoading,
  }
}
