import yaml from 'js-yaml'
import { useCallback, useEffect, useState } from 'react'

import { readProfileFile, saveProfileFile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { parseYamlSafe } from '@/utils/yaml'

type SeqConfig = Partial<Record<'prepend' | 'append' | 'delete', unknown>>

const toList = <T>(value: unknown): T[] =>
  Array.isArray(value) ? (value as T[]) : []

const anyItem = () => true

const isList = (value: unknown, isItem: (item: unknown) => boolean) =>
  value === undefined ||
  value === null ||
  (Array.isArray(value) && value.every(isItem))

const readSeqConfig = (
  input: string,
  isItem: (item: unknown) => boolean,
): SeqConfig | null | undefined => {
  const parsed = parseYamlSafe(input)
  if (parsed === null) return null
  if (typeof parsed !== 'object' || Array.isArray(parsed)) return undefined
  const config = parsed as SeqConfig
  return isList(config.prepend, isItem) &&
    isList(config.append, isItem) &&
    isList(config.delete, anyItem)
    ? config
    : undefined
}

interface SeqDocumentOptions {
  isItem?: (item: unknown) => boolean
  readDelete?: (value: unknown) => string[]
}

export const useSeqDocument = <T>(
  property: string,
  open: boolean,
  { isItem = anyItem, readDelete = toList }: SeqDocumentOptions = {},
) => {
  const [text, setText] = useState('')
  const [status, setStatus] = useState<'loading' | 'ready' | 'failed'>(
    'loading',
  )
  const [visualization, setVisualization] = useState(true)
  const [prependSeq, setPrependSeq] = useState<T[]>([])
  const [appendSeq, setAppendSeq] = useState<T[]>([])
  const [deleteSeq, setDeleteSeq] = useState<string[]>([])

  const applyText = useCallback(
    (input: string) => {
      const parsed = readSeqConfig(input, isItem)
      if (parsed === undefined) {
        showNotice.error(
          'profiles.page.feedback.notifications.editorBrokenYaml',
        )
        return false
      }
      setPrependSeq(toList<T>(parsed?.prepend))
      setAppendSeq(toList<T>(parsed?.append))
      setDeleteSeq(readDelete(parsed?.delete))
      return true
    },
    [isItem, readDelete],
  )

  useEffect(() => {
    if (!open) return
    let cancelled = false
    readProfileFile(property)
      .then((data) => {
        if (cancelled) return
        setText(data)
        setVisualization(applyText(data))
        setStatus('ready')
      })
      .catch((error) => {
        if (cancelled) return
        setStatus('failed')
        showNotice.error(error)
      })
    return () => {
      cancelled = true
    }
  }, [applyText, open, property])

  const dump = () =>
    yaml.dump(
      { prepend: prependSeq, append: appendSeq, delete: deleteSeq },
      { forceQuotes: true },
    )

  const toggleVisualization = () => {
    if (visualization) {
      setText(dump())
      setVisualization(false)
    } else if (applyText(text)) {
      setVisualization(true)
    }
  }

  const save = async () => {
    if (!(await saveProfileFile(property, visualization ? dump() : text))) {
      return false
    }
    showNotice.success('shared.feedback.notifications.saved')
    return true
  }

  return {
    loading: status === 'loading',
    ready: status === 'ready',
    text,
    setText,
    visualization,
    toggleVisualization,
    prependSeq,
    setPrependSeq,
    appendSeq,
    setAppendSeq,
    deleteSeq,
    setDeleteSeq,
    save,
  }
}
