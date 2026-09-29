import { useCallback, useEffect, useRef, useState } from 'react'

import { useVerge } from '@/hooks/use-verge'
import { useVisibility } from '@/hooks/use-visibility'
import { fitWindowToContent } from '@/services/cmds'

const FIT_DEBOUNCE_MS = 120

const RETRY_LIMIT_MS = 60_000

const COMPACT_HYSTERESIS = 24

const measureContentHeight = (root: HTMLElement) => {
  const previous = root.style.height
  root.style.height = 'auto'
  const height = root.scrollHeight
  root.style.height = previous
  return height
}

export const useFitWindowToContent = () => {
  const { verge } = useVerge()
  const enabled = verge?.window_fit_content !== false
  const visible = useVisibility()

  const [root, setRoot] = useState<HTMLElement | null>(null)
  const [compact, setCompact] = useState(false)

  const visibleRef = useRef(visible)

  const compactRef = useRef(false)
  const normalHeightRef = useRef(0)
  const compactSavingRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const chainRef = useRef(0)

  // true — человек тянет край окна, подгонку надо повторить позже.
  const applyFit = useCallback(async (): Promise<boolean> => {
    if (!visibleRef.current) return false
    if (!enabled) {
      if (compactRef.current) {
        compactRef.current = false
        setCompact(false)
      }
      return false
    }
    if (!root) return false

    const chrome = Math.max(0, window.innerHeight - root.clientHeight)
    const desired = measureContentHeight(root) + chrome
    if (desired <= 0) return false

    const ceiling = await fitWindowToContent(desired).catch(() => 0)
    if (ceiling === null) return true
    if (!ceiling) return false

    if (compactRef.current) {
      if (normalHeightRef.current > desired) {
        compactSavingRef.current = normalHeightRef.current - desired
      }
      const normalGuess = desired + compactSavingRef.current
      if (normalGuess + COMPACT_HYSTERESIS <= ceiling) {
        compactRef.current = false
        setCompact(false)
      }
      return false
    }

    normalHeightRef.current = desired
    if (desired > ceiling + 1) {
      compactRef.current = true
      setCompact(true)
    }
    return false
  }, [root, enabled])

  const schedule = useCallback(() => {
    const chain = ++chainRef.current
    const startedAt = Date.now()
    const run = () => {
      if (timerRef.current) clearTimeout(timerRef.current)
      timerRef.current = setTimeout(() => {
        timerRef.current = undefined
        void applyFit().then((later) => {
          // Повтор, пока человек тянет край, обрывают новый вызов подгона,
          // уход со страницы и слишком долгое ожидание — следующее изменение
          // содержимого заведёт подгон заново.
          const current = chain === chainRef.current
          if (later && current && Date.now() - startedAt < RETRY_LIMIT_MS) {
            run()
          }
        })
      }, FIT_DEBOUNCE_MS)
    }
    run()
  }, [applyFit])

  useEffect(() => {
    const wasVisible = visibleRef.current
    visibleRef.current = visible
    if (!visible || wasVisible) return
    schedule()
  }, [visible, schedule])

  useEffect(() => {
    if (!root) return

    const observer = new ResizeObserver(schedule)
    const observeAll = () => {
      observer.disconnect()
      observer.observe(root)
      for (const child of Array.from(root.children)) observer.observe(child)
    }
    observeAll()

    const mutations = new MutationObserver(() => {
      observeAll()
      schedule()
    })
    mutations.observe(root, { childList: true })

    schedule()

    return () => {
      observer.disconnect()
      mutations.disconnect()
      chainRef.current += 1
      if (timerRef.current) {
        clearTimeout(timerRef.current)
        timerRef.current = undefined
      }
    }
  }, [root, schedule])

  return { fitRef: setRoot, compact }
}
