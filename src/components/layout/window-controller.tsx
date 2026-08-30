import { Close, CropSquare, FilterNone, Minimize } from '@mui/icons-material'
import { Box, IconButton } from '@mui/material'
import { type PointerEvent, useCallback } from 'react'

import { useWindowControls } from '@/hooks/use-window'
import getSystem from '@/utils/get-system'

const RESIZE_HANDLES = [
  { direction: 'North', position: 'north' },
  { direction: 'NorthEast', position: 'north-east' },
  { direction: 'East', position: 'east' },
  { direction: 'SouthEast', position: 'south-east' },
  { direction: 'South', position: 'south' },
  { direction: 'SouthWest', position: 'south-west' },
  { direction: 'West', position: 'west' },
  { direction: 'NorthWest', position: 'north-west' },
] as const

export const WindowResizeHandles = () => {
  const { currentWindow, maximized } = useWindowControls()

  const startResizeDragging = useCallback(
    (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return

      event.preventDefault()
      const direction = event.currentTarget.dataset.resizeDirection
      const handle = RESIZE_HANDLES.find((item) => item.direction === direction)

      if (handle) {
        void currentWindow
          .startResizeDragging(handle.direction)
          .catch((error) =>
            console.warn(
              '[WindowResizeHandles] Не удалось изменить размер окна:',
              error,
            ),
          )
      }
    },
    [currentWindow],
  )

  if (getSystem() !== 'linux' || maximized) return null

  return (
    <div
      className="window-resize-handles"
      data-tauri-drag-region="false"
      aria-hidden="true"
    >
      {RESIZE_HANDLES.map(({ direction, position }) => (
        <div
          key={direction}
          className={`window-resize-handle window-resize-handle--${position}`}
          data-resize-direction={direction}
          onPointerDown={startResizeDragging}
        />
      ))}
    </div>
  )
}

export function WindowControls() {
  const OS = getSystem()
  const { maximized, minimize, close, toggleMaximize } = useWindowControls()

  return (
    <Box
      sx={{
        display: 'flex',
        gap: 1,
        alignItems: 'center',
        '> button': {
          cursor: 'default',
        },
      }}
    >
      {OS === 'macos' && (
        <>
          {/* Стиль macOS: закрыть → свернуть → развернуть */}
          <IconButton size="small" sx={{ fontSize: 14 }} onClick={close}>
            <Close fontSize="inherit" color="inherit" />
          </IconButton>
          <IconButton size="small" sx={{ fontSize: 14 }} onClick={minimize}>
            <Minimize fontSize="inherit" color="inherit" />
          </IconButton>
          <IconButton
            size="small"
            sx={{ fontSize: 14 }}
            onClick={toggleMaximize}
          >
            {maximized ? (
              <FilterNone fontSize="inherit" color="inherit" />
            ) : (
              <CropSquare fontSize="inherit" color="inherit" />
            )}
          </IconButton>
        </>
      )}

      {OS === 'windows' && (
        <>
          {/* Стиль Windows: свернуть → развернуть → закрыть */}
          <IconButton size="small" sx={{ fontSize: 16 }} onClick={minimize}>
            <Minimize fontSize="inherit" color="inherit" />
          </IconButton>
          <IconButton
            size="small"
            sx={{ fontSize: 16 }}
            onClick={toggleMaximize}
          >
            {maximized ? (
              <FilterNone fontSize="inherit" color="inherit" />
            ) : (
              <CropSquare fontSize="inherit" color="inherit" />
            )}
          </IconButton>
          <IconButton
            size="small"
            sx={{ fontSize: 16, ':hover': { bgcolor: 'red', color: 'white' } }}
            onClick={close}
          >
            <Close fontSize="inherit" color="inherit" />
          </IconButton>
        </>
      )}

      {OS === 'linux' && (
        <>
          {/* Типичная раскладка Linux-десктопа (в GNOME/KDE обычно: свернуть → развернуть → закрыть) */}
          <IconButton size="small" sx={{ fontSize: 16 }} onClick={minimize}>
            <Minimize fontSize="inherit" color="inherit" />
          </IconButton>
          <IconButton
            size="small"
            sx={{ fontSize: 16 }}
            onClick={toggleMaximize}
          >
            {maximized ? (
              <FilterNone fontSize="inherit" color="inherit" />
            ) : (
              <CropSquare fontSize="inherit" color="inherit" />
            )}
          </IconButton>
          <IconButton
            size="small"
            sx={{ fontSize: 16, ':hover': { bgcolor: 'red', color: 'white' } }}
            onClick={close}
          >
            <Close fontSize="inherit" color="inherit" />
          </IconButton>
        </>
      )}
    </Box>
  )
}
