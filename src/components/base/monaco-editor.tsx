import { Suspense, lazy } from 'react'

import { loadMonacoEditor } from '@/services/monaco'
import { useThemeMode } from '@/services/states'
import getSystem from '@/utils/get-system'

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

const SCROLLBAR = {
  verticalScrollbarSize: 8,
  horizontalScrollbarSize: 8,
  useShadows: false,
} as const

const EDITOR_OPTIONS = {
  tabSize: 2,
  mouseWheelZoom: true,
  quickSuggestions: { strings: true, comments: true, other: true },
  fontFamily: `Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${
    getSystem() === 'windows' ? ', twemoji mozilla' : ''
  }`,
  fontLigatures: false,
  smoothScrolling: true,
} as const

export const MonacoEditor = ({ options, ...props }: MonacoEditorProps) => {
  const themeMode = useThemeMode()
  return (
    <Suspense fallback={null}>
      <MonacoEditorContent
        theme={themeMode === 'light' ? 'light' : 'vs-dark'}
        {...props}
        options={{
          ...EDITOR_OPTIONS,
          minimap: { enabled: document.documentElement.clientWidth >= 1500 },
          ...options,
          scrollbar: { ...SCROLLBAR, ...options?.scrollbar },
        }}
      />
    </Suspense>
  )
}
