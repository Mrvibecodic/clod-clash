import { Button } from '@mui/material'
import { useEffect, useRef } from 'react'

import { DialogRef } from '@/components/base'
import { useUpdate } from '@/hooks/use-update'

import { UpdateViewer } from '../setting/mods/update-viewer'

interface Props {
  className?: string
}

// clod: сайдбар, где живёт эта кнопка, отрисовывается с display:none —
// сама по себе она невидима. Поэтому найденное обновление показываем
// авто-открытием диалога; каждый номер версии предлагаем один раз за
// запуск приложения, чтобы закрытый диалог не выскакивал снова.
const offeredVersions = new Set<string>()

export const UpdateButton = (props: Props) => {
  const { className } = props
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

  return (
    <>
      <UpdateViewer ref={viewerRef} />

      <Button
        color="error"
        variant="contained"
        size="small"
        className={className}
        onClick={() => viewerRef.current?.open()}
      >
        New
      </Button>
    </>
  )
}
