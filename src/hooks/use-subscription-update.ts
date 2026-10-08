import { useCallback, useSyncExternalStore } from 'react'

import { useTauriEvent } from '@/hooks/use-listen'
import { useProfiles } from '@/hooks/use-profiles'
import { getUpdatingProfiles, updateProfile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { revalidateQueries } from '@/services/query-client'
import {
  createUpdatesStore,
  type UpdatesInFlight,
} from '@/utils/subscription-status'

/** Обновление подписки руками — не чаще, панель дёргать незачем. */
const REFRESH_COOLDOWN_MS = 30_000

/** Подписки на паузе после ручного обновления — одна пауза на всё окно. */
const paused = new Map<string, ReturnType<typeof setTimeout>>()
const pauseListeners = new Set<() => void>()

const pauseChanged = () => {
  for (const listener of pauseListeners) listener()
}

const subscribeToPause = (listener: () => void) => {
  pauseListeners.add(listener)
  return () => {
    pauseListeners.delete(listener)
  }
}

const pause = (uid: string) => {
  clearTimeout(paused.get(uid))
  paused.set(
    uid,
    setTimeout(() => {
      paused.delete(uid)
      pauseChanged()
    }, REFRESH_COOLDOWN_MS),
  )
  pauseChanged()
}

/** «Идёт обновление» по подпискам — одно на всё окно. */
const updates = createUpdatesStore()

const applySnapshot = (snapshot: UpdatesInFlight) => {
  // Обновление закончилось — список подписок мог смениться.
  if (updates.apply(snapshot).length > 0) {
    void revalidateQueries([['getProfiles']])
  }
}

const syncWithBackend = async () => {
  const snapshot = await getUpdatingProfiles().catch(() => null)
  if (snapshot) applySnapshot(snapshot)
}

/** Свой вызов обновления: подписка занята, пока он не вернулся. */
export const beginOwnUpdate = updates.beginOwn
export const endOwnUpdate = updates.endOwn
export const isSubscriptionUpdating = updates.isUpdating

export const useSubscriptionUpdating = (uid?: string) =>
  useSyncExternalStore(
    updates.subscribe,
    () => !!uid && updates.isUpdating(uid),
  )

/**
 * clod: «идёт обновление» по подписке — со слов бэкенда. Любая смена набора
 * обновляющихся подписок, кнопкой или расписанием, приходит снимком с номером;
 * тот же снимок отдаёт сверка при появлении окна (после того, как слушатель
 * встал) и при его показе — события могли пройти мимо окна (выход, потом
 * отменённый; окно пересоздано). Слушается один раз, в каркасе окна.
 */
export const useSubscriptionUpdateEvents = () => {
  useTauriEvent<UpdatesInFlight>(
    'clod://profiles-updating',
    ({ payload }) => applySnapshot(payload),
    () => void syncWithBackend(),
  )

  useTauriEvent('verge://window-shown', () => void syncWithBackend())
}

/**
 * clod: кнопка «Обновить подписку». Пока подписка обновляется (кем угодно),
 * кнопка занята; после удачного ручного обновления — пауза 30 с на подписку
 * для всех кнопок сразу, и на её время кнопки неактивны.
 */
export const useSubscriptionUpdate = (uid?: string) => {
  const { mutateProfiles } = useProfiles()
  const updating = useSubscriptionUpdating(uid)
  const isPaused = useSyncExternalStore(
    subscribeToPause,
    () => !!uid && paused.has(uid),
  )

  const refresh = useCallback(async () => {
    if (!uid || updates.isUpdating(uid) || paused.has(uid)) return
    updates.beginOwn(uid)
    try {
      await updateProfile(uid)
      pause(uid)
      await mutateProfiles()
      showNotice.success('home.components.subscription.updated')
    } catch (error) {
      showNotice.error(error)
    } finally {
      updates.endOwn(uid)
    }
  }, [uid, mutateProfiles])

  return { updating, paused: isPaused, refresh }
}
