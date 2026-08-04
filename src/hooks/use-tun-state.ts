import { getTunState } from '@/services/cmds'
import { useQuery } from '@/services/query-client'

import { useVisibility } from './use-visibility'

const defaultTunState: ITunState = {
  desired: false,
  active: false,
  capable: false,
  setup_declined: false,
}

/**
 * clod:tun-ready — состояние TUN так, как его видит бэкенд.
 *
 * Интерфейсу мало флага из конфига: он говорит, чего хочет пользователь, а не
 * что происходит. `active` — это то, что реально подано ядру (желание есть и
 * режим не подавлен), поэтому переключатель на экране больше не может гореть
 * над мёртвым туннелем.
 */
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
    /** Пользователь хочет TUN. */
    tunDesired: tun.desired,
    /** TUN реально работает. */
    tunActive: tun.active,
    /** Прав хватает: служба отвечает или приложение привилегировано. */
    tunCapable: tun.capable,
    /** Автонастройку службы на этой версии уже пробовали. */
    tunSetupDeclined: tun.setup_declined,
    /** Хотели, но не работает — это и есть повод показать подсказку. */
    tunBroken: tun.desired && !tun.active,
    mutateTunState,
    isLoading,
  }
}
