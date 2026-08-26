import {
  type EventCallback,
  type UnlistenFn,
  listen,
} from '@tauri-apps/api/event'
import { useEffect, useRef } from 'react'

export const useTauriEvent = <T>(
  eventName: string,
  handler: EventCallback<T>,
) => {
  const handlerRef = useRef(handler)

  useEffect(() => {
    handlerRef.current = handler
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

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [eventName])
}
