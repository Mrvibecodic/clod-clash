import { useEffect, useRef } from 'react'

import { DialogRef } from '@/components/base'
import { useUpdate } from '@/hooks/use-update'

import { UpdateViewer } from '../setting/mods/update-viewer'

// clod: найденное обновление показываем авто-открытием диалога; каждый
// номер версии предлагаем один раз за запуск приложения, чтобы закрытый
// диалог не выскакивал снова.
const offeredVersions = new Set<string>()

export const UpdateButton = () => {
  const viewerRef = useRef<DialogRef>(null)

  const { updateInfo } = useUpdate()

  useEffect(() => {
    if (!updateInfo?.available) return
    const version = updateInfo.version || 'unknown'
    if (offeredVersions.has(version)) return
    offeredVersions.add(version)
    viewerRef.current?.open()
  }, [updateInfo])

  if (!updateInfo?.available) return null

  return <UpdateViewer ref={viewerRef} />
}
