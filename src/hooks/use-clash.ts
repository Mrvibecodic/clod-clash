import { useLockFn } from 'ahooks'
import { useTranslation } from 'react-i18next'
import { getVersion } from 'tauri-plugin-mihomo-api'

import {
  getClashInfo,
  getRuntimeConfig,
  patchClashConfig,
} from '@/services/cmds'
import {
  getCacheData,
  revalidateQuery,
  setCacheData,
  useQuery,
} from '@/services/query-client'

type MutateClashUpdater =
  | ((old: IConfigData | undefined) => IConfigData | undefined)
  | IConfigData
  | undefined

const PORT_KEYS = [
  'port',
  'socks-port',
  'mixed-port',
  'redir-port',
  'tproxy-port',
] as const

type ClashInfoPatch = Partial<
  Pick<
    IConfigData,
    | 'port'
    | 'socks-port'
    | 'mixed-port'
    | 'redir-port'
    | 'tproxy-port'
    | 'external-controller'
    | 'secret'
  >
>

const hasClashInfoPayload = (patch: ClashInfoPatch) =>
  PORT_KEYS.some((key) => patch[key] != null) ||
  patch['external-controller'] != null ||
  patch.secret != null

const MIN_PORT = 1000
const MAX_PORT = 65535

type Translate = ReturnType<typeof useTranslation>['t']

const validatePortRange = (port: number, t: Translate) => {
  if (port < MIN_PORT) {
    throw new Error(
      t('settings.modals.clashPort.messages.portTooLow', { min: MIN_PORT }),
    )
  }
  if (port > MAX_PORT) {
    throw new Error(
      t('settings.modals.clashPort.messages.portTooHigh', { max: MAX_PORT }),
    )
  }
}

const validatePorts = (patch: ClashInfoPatch, t: Translate) => {
  PORT_KEYS.forEach((key) => {
    const port = patch[key]
    if (!port) return
    validatePortRange(port, t)
  })
}

export const useRuntimeConfig = (shouldFetch: boolean = true) => {
  return useQuery({
    queryKey: ['getRuntimeConfig'],
    queryFn: getRuntimeConfig,
    enabled: shouldFetch,
  })
}

export const useClash = () => {
  const { data: clash, refetch } = useRuntimeConfig()

  const { data: versionData, refetch: mutateVersion } = useQuery({
    queryKey: ['getVersion'],
    queryFn: getVersion,
  })

  const mutateClash = (updater?: MutateClashUpdater, revalidate?: boolean) => {
    if (updater === undefined) {
      return refetch()
    }
    const next =
      typeof updater === 'function'
        ? updater(getCacheData<IConfigData>(['getRuntimeConfig']))
        : updater
    setCacheData(['getRuntimeConfig'], next)
    if (revalidate !== false) {
      return refetch()
    }
    return Promise.resolve()
  }

  const patchClash = useLockFn(async (patch: Partial<IConfigData>) => {
    await patchClashConfig(patch)
    mutateClash()
  })

  const version = versionData?.meta
    ? `${versionData.version} Mihomo`
    : versionData?.version || '-'

  return {
    clash,
    version,
    mutateClash,
    mutateVersion,
    patchClash,
  }
}

export const useClashInfo = () => {
  const { t } = useTranslation()
  const { data: clashInfo, refetch: mutateInfo } = useQuery({
    queryKey: ['getClashInfo'],
    queryFn: getClashInfo,
  })

  const patchInfo = useLockFn(async (patch: ClashInfoPatch) => {
    if (!hasClashInfoPayload(patch)) return

    validatePorts(patch, t)

    await patchClashConfig(patch)
    mutateInfo()
    revalidateQuery(['getClashConfig'])
  })

  const invalidateClashConfig = () => revalidateQuery(['getClashConfig'])

  return {
    clashInfo,
    mutateInfo,
    patchInfo,
    invalidateClashConfig,
  }
}
