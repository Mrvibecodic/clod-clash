/* eslint-disable @eslint-react/set-state-in-effect */
import { useEffect } from 'foxact/use-abortable-effect'
import { useCallback, useState } from 'react'

import type { EditorReadFailure } from '@/utils/editor-save'

interface UseEditorDocumentOptions {
  open: boolean
  load: () => Promise<string>
}

export const useEditorDocument = ({ open, load }: UseEditorDocumentOptions) => {
  const [value, setValue] = useState('')
  const [savedValue, setSavedValue] = useState('')
  const [loading, setLoading] = useState(true)
  const [failure, setFailure] = useState<EditorReadFailure | null>(null)
  const [attempt, setAttempt] = useState(0)

  const resetDocumentState = useCallback(() => {
    setValue('')
    setSavedValue('')
    setLoading(true)
    setFailure(null)
  }, [])

  const applyLoadedValue = useCallback(
    (nextValue: string | null | undefined) => {
      const normalized = nextValue ?? ''
      setValue(normalized)
      setSavedValue(normalized)
    },
    [],
  )

  useEffect(
    (signal) => {
      resetDocumentState()

      if (!open) return

      load()
        .then((nextValue) => {
          if (signal.aborted) return
          applyLoadedValue(nextValue)
        })
        .catch((error) => {
          if (!signal.aborted) setFailure({ reason: error })
        })
        .finally(() => {
          if (!signal.aborted) setLoading(false)
        })
    },
    [applyLoadedValue, attempt, load, open, resetDocumentState],
  )

  const retry = useCallback(() => setAttempt((n) => n + 1), [])

  const markSaved = useCallback((nextValue: string) => {
    setSavedValue(nextValue)
  }, [])

  const dirty = value !== savedValue

  return {
    value,
    setValue,
    loading,
    failure,
    retry,
    dirty,
    markSaved,
  }
}
