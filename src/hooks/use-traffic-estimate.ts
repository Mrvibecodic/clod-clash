import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import useSWR from 'swr'

import { useProfiles } from '@/hooks/use-profiles'
import { useVisibility } from '@/hooks/use-visibility'
import { getTrafficEstimate, updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'

/** Как часто перечитываем счёт из бэкенда. */
const POLL_INTERVAL_MS = 10_000
/**
 * Ниже этого порога досчитанное клиентом не стоит упоминания: значение из
 * подписки и так показывает ту же цифру, а лишний треугольник только пугает.
 */
const MIN_VISIBLE_BYTES = 10 * 1024 * 1024
/** Обновление подписки руками — не чаще, панель дёргать незачем. */
const REFRESH_COOLDOWN_MS = 30_000

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
  const { mutateProfiles } = useProfiles()
  const visible = useVisibility()
  const [refreshing, setRefreshing] = useState(false)
  const lastRefreshRef = useRef(0)

  const uid = profile?.uid
  const extra = profile?.extra
  // clod: свёрнутое в трей приложение не опрашивает бэкенд и не перерисовывает
  // карточку — счёт всё равно ведётся в бэкенде, а показывать его некому.
  // Собственная проверка видимости, а не `refreshWhenHidden` у SWR: тот знает
  // только про `document.hidden`, а окно уезжает в трей целиком.
  const { data, mutate } = useSWR(
    uid && extra ? ['trafficEstimate', uid] : null,
    getTrafficEstimate,
    {
      refreshInterval: visible ? POLL_INTERVAL_MS : 0,
      revalidateOnFocus: false,
    },
  )

  // Показали окно — сразу свежее число, а не то, что застыло при сворачивании.
  useEffect(() => {
    if (visible) void mutate()
  }, [visible, mutate])

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

  const refresh = useCallback(async () => {
    if (!uid) return
    const now = Date.now()
    if (refreshing || now - lastRefreshRef.current < REFRESH_COOLDOWN_MS) return
    lastRefreshRef.current = now
    setRefreshing(true)
    try {
      await updateProfile(uid)
      await mutateProfiles()
    } catch (error) {
      showNotice.error(error)
    } finally {
      setRefreshing(false)
    }
  }, [uid, refreshing, mutateProfiles])

  return { estimate, refreshing, refresh }
}
