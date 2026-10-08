import {
  type EventCallback,
  type UnlistenFn,
  listen,
} from '@tauri-apps/api/event'
import { useEffect, useRef } from 'react'

/**
 * `onReady` — когда слушатель встал (или встать не смог): то, что случилось
 * раньше, событием уже не придёт, и спрашивать о нём надо после этого.
 */
export const useTauriEvent = <T>(
  eventName: string,
  handler: EventCallback<T>,
  onReady?: () => void,
) => {
  const handlerRef = useRef(handler)
  const onReadyRef = useRef(onReady)

  useEffect(() => {
    handlerRef.current = handler
    onReadyRef.current = onReady
  })

  useEffect(() => {
    let disposed = false
    let unlisten: UnlistenFn | undefined

    listen<T>(eventName, (event) => handlerRef.current(event))
      .then((result) => {
        if (disposed) {
          result()
        } else {
          unlisten = result
        }
      })
      .catch((error) =>
        console.error(
          `[useTauriEvent] ${eventName}: registration failed:`,
          error,
        ),
      )
      .finally(() => {
        if (!disposed) onReadyRef.current?.()
      })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [eventName])
}
