import { useEffect, useRef } from 'react'

import { hideInitialOverlay } from '../utils'

export const useLoadingOverlay = () => {
  const doneRef = useRef(false)

  useEffect(() => {
    if (doneRef.current) return
    doneRef.current = true

    const timer = hideInitialOverlay()
    return () => {
      if (timer !== undefined) window.clearTimeout(timer)
    }
  }, [])
}
