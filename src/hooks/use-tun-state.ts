import { getTunState } from '@/services/cmds'
import { useQuery } from '@/services/query-client'

const defaultTunState: ITunState = {
  desired: false,
  active: false,
  capable: false,
  setup_declined: false,
  needs_repair: false,
  service_serves_this_copy: true,
  runtime_stack: null,
  failure: null,
}

/** Как часто окно перечитывает состояние TUN, пока оно на экране. */
export const TUN_STATE_POLL_MS = 10_000

export const TUN_STATE_QUERY = {
  queryKey: ['getTunState'],
  queryFn: getTunState,
} as const

/**
 * Опрос заведён один раз на окно — в `AppDataProvider`. Хук живёт в десятке
 * мест сразу, а таймер опроса у SWR свой у каждого экземпляра: интервал здесь
 * означал бы столько же параллельных опросов со своими фазами.
 *
 * `refetchInterval: false` — не «без опроса», а «запрос с опросом»: отказ
 * оставляет прошлые данные и в кэш не попадает. Ошибка в кэше остановила бы
 * общий опрос, а перечитывание по событиям бэкенда идёт загрузчиком первого
 * подписавшегося экземпляра — то есть одного из хуков.
 */
export const useTunState = () => {
  const { data: tun = defaultTunState, refetch: mutateTunState } = useQuery({
    ...TUN_STATE_QUERY,
    refetchInterval: false,
    refetchOnWindowFocus: true,
    refetchOnReconnect: true,
  })

  return {
    tunDesired: tun.desired,
    tunActive: tun.active,
    tunCapable: tun.capable,
    tunNeedsRepair: tun.needs_repair,
    tunServiceServesThisCopy: tun.service_serves_this_copy,
    tunBroken: tun.desired && !tun.active,
    // Туннель поднят, но проба не прошла: устройство есть, а трафик через него
    // не идёт — стоит или уходит мимо.
    tunNoTraffic:
      tun.desired &&
      tun.active &&
      (tun.failure === 'noTraffic' || tun.failure === 'bypassed'),
    tunRuntimeStack: tun.runtime_stack ?? null,
    tunFailure: tun.failure ?? null,
    mutateTunState,
  }
}
