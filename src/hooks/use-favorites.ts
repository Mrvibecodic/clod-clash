import { useLockFn, useMemoizedFn } from 'ahooks'
import { useMemo } from 'react'

import { useProfiles } from '@/hooks/use-profiles'
import { showNotice } from '@/services/notice-service'

export const favoritesFirst = <T extends { name: string }>(
  items: T[],
  favorites: Set<string>,
) =>
  favorites.size === 0
    ? items
    : [
        ...items.filter((item) => favorites.has(item.name)),
        ...items.filter((item) => !favorites.has(item.name)),
      ]

export const useFavorites = () => {
  const { current, patchCurrent } = useProfiles()
  const favorites = useMemo(
    () => new Set(current?.favorites ?? []),
    [current?.favorites],
  )

  const toggleFavorite = useMemoizedFn(
    useLockFn(async (nodeName: string) => {
      if (!current?.uid) return
      const stored = current.favorites ?? []
      const next = favorites.has(nodeName)
        ? stored.filter((name) => name !== nodeName)
        : [...stored, nodeName]
      try {
        await patchCurrent({ favorites: next })
      } catch (error) {
        showNotice.error(error)
      }
    }),
  )

  return { favorites, toggleFavorite }
}
