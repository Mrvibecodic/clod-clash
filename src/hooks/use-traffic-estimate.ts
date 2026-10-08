import { useMemo } from 'react'
import useSWR from 'swr'

import { useRefreshOnReturn } from '@/hooks/use-refresh-on-return'
import { useSubscriptionUpdate } from '@/hooks/use-subscription-update'
import { getTrafficEstimate } from '@/services/cmds'
import { revalidateQuery } from '@/services/query-client'

/** Как часто перечитываем счёт из бэкенда. */
const POLL_INTERVAL_MS = 10_000
/**
 * Ниже этого порога досчитанное клиентом не стоит упоминания: значение из
 * подписки и так показывает ту же цифру, а лишний треугольник только пугает.
 */
const MIN_VISIBLE_BYTES = 10 * 1024 * 1024

interface Estimate {
  /** Байты, досчитанные клиентом поверх подписки (0 — показывать нечего). */
  localBytes: number
  /** Показывать ли пометку «примерно». */
  approximate: boolean
  /** Unix-секунды: когда данные подписки были точными. */
  baselineAt: number
}

const EMPTY: Estimate = { localBytes: 0, approximate: false, baselineAt: 0 }

/**
 * clod: расход трафика между обновлениями подписки.
 *
 * Панель пересчитывает расход не чаще раза в час, поэтому клиент досчитывает
 * прошедшее через прокси сам. Счёт применяется, только если он снят с той же
 * базы, что сейчас в профиле: иначе бэкенд ещё не успел свериться с новой
 * подпиской, и складывать эти числа значило бы посчитать трафик дважды.
 *
 * Возвращаемое значение годится только для показа. Логика «трафик
 * закончился», критические состояния и кнопки продления считаются строго по
 * данным подписки.
 */
export const useTrafficEstimate = (profile?: IProfileItem) => {
  const uid = profile?.uid
  const { updating: refreshing, paused, refresh } = useSubscriptionUpdate(uid)
  const extra = profile?.extra
  const visible = useRefreshOnReturn(
    () => uid && revalidateQuery(['trafficEstimate', uid]),
  )
  // clod: свёрнутое в трей приложение не опрашивает бэкенд и не перерисовывает
  // карточку — счёт всё равно ведётся в бэкенде, а показывать его некому.
  // Собственная проверка видимости, а не `refreshWhenHidden` у SWR: тот знает
  // только про `document.hidden`, а окно уезжает в трей целиком.
  const { data } = useSWR(
    uid && extra ? ['trafficEstimate', uid] : null,
    getTrafficEstimate,
    {
      refreshInterval: visible ? POLL_INTERVAL_MS : 0,
      revalidateOnFocus: false,
    },
  )

  const estimate = useMemo<Estimate>(() => {
    if (!data || !uid || !extra) return EMPTY
    const sameBaseline =
      data.profile === uid &&
      data.baselineUpload === extra.upload &&
      data.baselineDownload === extra.download
    if (!sameBaseline) return EMPTY
    const localBytes = Math.max(0, data.localBytes)
    return {
      localBytes,
      approximate: localBytes >= MIN_VISIBLE_BYTES,
      baselineAt: data.baselineAt,
    }
  }, [data, uid, extra])

  return { estimate, refreshing, paused, refresh }
}
