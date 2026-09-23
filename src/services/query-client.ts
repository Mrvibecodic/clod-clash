import { SWRConfig, unstable_serialize } from 'swr'
import useSWR, {
  type SWRConfiguration,
  type SWRResponse,
  mutate as swrMutate,
} from 'swr'

type QueryKey = string | readonly unknown[]
type QueryDataUpdater<T> =
  | T
  | undefined
  | ((current: T | undefined) => T | undefined)

type QueryOptions<T> = {
  queryKey: QueryKey
  queryFn: () => Promise<T> | T
  enabled?: boolean
  initialData?: T | (() => T | undefined)
  placeholderData?: T | (() => T | undefined)
  staleTime?: number
  retry?: number | false
  retryDelay?: number | ((attempt: number) => number)
  refetchInterval?: number | false
  refetchIntervalInBackground?: boolean
  revalidateOnMount?: boolean
  refetchOnWindowFocus?: boolean
  refetchOnReconnect?: boolean
}

type QueryResult<T> = SWRResponse<T> & {
  isFetching: boolean
  isPending: boolean
  refetch: () => Promise<{ data: T | undefined }>
}

const serializeQueryKey = (queryKey: QueryKey) => unstable_serialize(queryKey)

/**
 * Начальные данные запроса (`initialData`,
 * `placeholderData`) до первой загрузки: SWR показывает их, но у себя в кэше
 * не держит. Всё остальное живёт в кэше SWR, по которому рисуется экран, и
 * читается оттуда же. Раньше здесь было зеркало всего кэша с потолком ключей:
 * составные ключи значков вытесняли из него настройки ядра, и оптимистичная
 * правка сливалась с пустотой. Начальные данные есть у считанных запросов —
 * расти здесь нечему.
 */
const fallbackCache = new Map<string, unknown>()

export const swrConfig: SWRConfiguration = {
  dedupingInterval: 2000,
  errorRetryCount: 3,
  errorRetryInterval: 5000,
  revalidateOnFocus: false,
}

export const getCacheData = <T>(queryKey: QueryKey): T | undefined => {
  const cacheKey = serializeQueryKey(queryKey)
  const shown = SWRConfig.defaultValue.cache.get(cacheKey)?.data as
    | T
    | undefined
  return shown ?? (fallbackCache.get(cacheKey) as T | undefined)
}

const nextCacheData = <T>(
  queryKey: QueryKey,
  updaterOrData: QueryDataUpdater<T>,
) => {
  if (typeof updaterOrData !== 'function') return updaterOrData
  const update = updaterOrData as (current: T | undefined) => T | undefined
  return update(getCacheData<T>(queryKey))
}

export const setCacheData = <T>(
  queryKey: QueryKey,
  updaterOrData: QueryDataUpdater<T>,
) => {
  const next = nextCacheData(queryKey, updaterOrData)
  void swrMutate(queryKey, next, {
    populateCache: true,
    revalidate: false,
  })
  return next
}

export const setCacheDataAsync = async <T>(
  queryKey: QueryKey,
  updaterOrData: QueryDataUpdater<T>,
) => {
  const next = nextCacheData(queryKey, updaterOrData)
  await swrMutate(queryKey, next, {
    populateCache: true,
    revalidate: false,
  })
  return next
}

export const revalidateQuery = (queryKey: QueryKey) => swrMutate(queryKey)

export const revalidateQueries = (queryKeys: readonly QueryKey[]) =>
  Promise.all(queryKeys.map(revalidateQuery))

export const revalidateQueriesByPrefix = (prefixes: readonly string[]) =>
  swrMutate((key) => prefixes.includes(Array.isArray(key) ? key[0] : key))

export const removeCacheData = (queryKey: QueryKey) => {
  fallbackCache.delete(serializeQueryKey(queryKey))
  return swrMutate(queryKey, undefined, {
    populateCache: true,
    revalidate: false,
  })
}

export const fetchCacheData = async <T>(
  queryKey: QueryKey,
  queryFn: () => Promise<T> | T,
) => {
  const data = await queryFn()
  setCacheData(queryKey, data)
  return data
}

export function useQuery<T>(options: QueryOptions<T>): QueryResult<T> {
  const {
    queryKey,
    queryFn,
    enabled = true,
    initialData,
    placeholderData,
    retry,
    retryDelay,
    refetchInterval,
    refetchIntervalInBackground,
    revalidateOnMount,
    refetchOnWindowFocus,
    refetchOnReconnect,
    staleTime,
  } = options

  const fallbackDataSource = initialData ?? placeholderData
  const fallbackData =
    typeof fallbackDataSource === 'function'
      ? (fallbackDataSource as () => T | undefined)()
      : fallbackDataSource
  const serializedKey = serializeQueryKey(queryKey)
  if (
    enabled &&
    fallbackData !== undefined &&
    !fallbackCache.has(serializedKey)
  ) {
    fallbackCache.set(serializedKey, fallbackData)
  }

  const swr = useSWR<T>(enabled ? queryKey : null, queryFn, {
    ...(staleTime !== undefined && { dedupingInterval: staleTime }),
    ...(retry !== undefined && {
      errorRetryCount: retry === false ? 0 : retry,
    }),
    ...(typeof retryDelay === 'number' && { errorRetryInterval: retryDelay }),
    fallbackData,
    keepPreviousData: placeholderData !== undefined,
    onErrorRetry: (_error, _key, config, revalidate, { retryCount }) => {
      const maxRetries = config.errorRetryCount
      if (maxRetries !== undefined && retryCount > maxRetries) return

      const interval =
        typeof retryDelay === 'function'
          ? retryDelay(Math.max(retryCount - 1, 0))
          : config.errorRetryInterval

      setTimeout(() => {
        revalidate({ retryCount, dedupe: true })
      }, interval)
    },
    revalidateOnFocus: refetchOnWindowFocus,
    revalidateOnMount,
    revalidateOnReconnect: refetchOnReconnect ?? false,
    refreshInterval: refetchInterval || 0,
    refreshWhenHidden: refetchIntervalInBackground ?? false,
  })

  return {
    ...swr,
    isFetching: swr.isValidating,
    isPending: swr.isLoading,
    refetch: async () => ({ data: await swr.mutate() }),
  }
}
