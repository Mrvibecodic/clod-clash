import { Suspense, lazy } from 'react'

import { loadMonacoEditor } from '@/services/monaco'

type MonacoEditorProps = import('@monaco-editor/react').EditorProps

let monacoEditorBundle: Awaited<ReturnType<typeof loadMonacoEditor>>

const MonacoEditorView = ({ beforeMount, ...props }: MonacoEditorProps) => {
  const { Editor, beforeEditorMount } = monacoEditorBundle

  return (
    <Editor
      {...props}
      beforeMount={(monaco) => {
        beforeEditorMount()
        beforeMount?.(monaco)
      }}
    />
  )
}

const MonacoEditorContent = lazy(async () => {
  monacoEditorBundle = await loadMonacoEditor()
  return { default: MonacoEditorView }
})

/** Полосы прокрутки — как у остального окна: 8 px, без теней (цвет — в теме). */
const SCROLLBAR = {
  verticalScrollbarSize: 8,
  horizontalScrollbarSize: 8,
  useShadows: false,
} as const

export const MonacoEditor = ({ options, ...props }: MonacoEditorProps) => (
  <Suspense fallback={null}>
    <MonacoEditorContent
      {...props}
      options={{
        ...options,
        scrollbar: { ...SCROLLBAR, ...options?.scrollbar },
      }}
    />
  </Suspense>
)
