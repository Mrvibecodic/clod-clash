import {
  CloseFullscreenRounded,
  ContentPasteRounded,
  FormatPaintRounded,
  OpenInFullRounded,
} from '@mui/icons-material'
import {
  Button,
  ButtonGroup,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  IconButton,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { type ReactNode, useCallback, useEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'

import { BaseLoadingOverlay, MonacoEditor } from '@/components/base'
import { useWindowControls } from '@/hooks/use-window'
import { showNotice } from '@/services/notice-service'
import type { MonacoEditorInstance } from '@/types/monaco'
import { dismissUnlessDirty } from '@/utils/dialog-dismiss'

type EditorLanguage = 'yaml' | 'javascript'

export interface EditorViewerProps {
  open: boolean
  title?: string | ReactNode
  value: string
  language: EditorLanguage
  path: string
  loading?: boolean
  dirty?: boolean
  onChange?: (value: string) => void
  onSave?: () => void | boolean | Promise<void | boolean>
  onClose: () => void
}

export const EditorViewer = ({
  open,
  title,
  value,
  language,
  path,
  loading = false,
  dirty,
  onChange,
  onSave,
  onClose,
}: EditorViewerProps) => {
  const { t } = useTranslation()
  const { maximized: isMaximized, toggleMaximize } = useWindowControls()
  const editorRef = useRef<MonacoEditorInstance | null>(null)

  const resolvedTitle = title ?? t('profiles.components.menu.editFile')
  const disableSave = loading || dirty === false

  const syncEditorValue = useCallback(() => {
    const model = editorRef.current?.getModel()
    if (model && model.getValue() !== value) {
      model.setValue(value)
    }
  }, [value])

  const handleSave = useLockFn(async () => {
    try {
      if ((await onSave?.()) === false) return
      onClose()
    } catch (error) {
      showNotice.error(error)
    }
  })

  const handleClose = () => {
    try {
      onClose()
    } catch (error) {
      showNotice.error(error)
    }
  }

  const handlePaste = useLockFn(async () => {
    try {
      if (loading || !editorRef.current) return

      const text = await navigator.clipboard.readText()
      if (!text) return

      const editorInstance = editorRef.current
      const model = editorInstance.getModel()
      const selections = editorInstance.getSelections()
      if (!model || !selections || selections.length === 0) return

      editorInstance.pushUndoStop()
      editorInstance.executeEdits(
        'explicit-paste',
        selections.map((selection) => ({
          range: selection,
          text,
          forceMoveMarkers: true,
        })),
      )
      editorInstance.pushUndoStop()
      editorInstance.focus()
    } catch (error) {
      showNotice.error(error)
    }
  })

  const handleFormat = useLockFn(async () => {
    try {
      if (loading) return
      await editorRef.current?.getAction('editor.action.formatDocument')?.run()
    } catch (error) {
      showNotice.error(error)
    }
  })

  const handleToggleMaximize = useLockFn(async () => {
    try {
      await toggleMaximize()
    } catch (error) {
      showNotice.error(error)
    }
  })

  useEffect(() => {
    if (!open || loading) return
    syncEditorValue()
  }, [loading, open, syncEditorValue])

  useEffect(() => {
    return () => {
      editorRef.current?.dispose()
      editorRef.current = null
    }
  }, [])

  return (
    <Dialog
      open={open}
      onClose={dismissUnlessDirty(dirty, handleClose)}
      maxWidth="xl"
      fullWidth
      disableEnforceFocus
    >
      <DialogTitle>{resolvedTitle}</DialogTitle>

      <DialogContent
        sx={{
          width: 'auto',
          height: 'calc(100vh - 185px)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        <div style={{ position: 'relative', flex: '1 1 auto', minHeight: 0 }}>
          <BaseLoadingOverlay isLoading={loading} />
          {!loading && (
            <MonacoEditor
              height="100%"
              path={path}
              value={value}
              language={language}
              loading={null}
              saveViewState
              keepCurrentModel={false}
              onMount={(editorInstance) => {
                editorRef.current = editorInstance
                syncEditorValue()
              }}
              onChange={(nextValue) => onChange?.(nextValue ?? '')}
              options={{
                automaticLayout: true,
                renderValidationDecorations: 'on',
                padding: {
                  top: 33,
                },
              }}
            />
          )}
        </div>

        <ButtonGroup
          variant="contained"
          sx={{ position: 'absolute', left: '14px', bottom: '8px' }}
        >
          <IconButton
            size="medium"
            color="inherit"
            title={t('profiles.page.importForm.actions.paste')}
            disabled={loading}
            onClick={() => {
              void handlePaste()
            }}
          >
            <ContentPasteRounded fontSize="inherit" />
          </IconButton>
          <IconButton
            size="medium"
            color="inherit"
            title={t('profiles.modals.editor.actions.format')}
            disabled={loading}
            onClick={() => {
              void handleFormat()
            }}
          >
            <FormatPaintRounded fontSize="inherit" />
          </IconButton>
          <IconButton
            size="medium"
            color="inherit"
            title={t(
              isMaximized ? 'shared.window.restore' : 'shared.window.maximize',
            )}
            onClick={() => {
              void handleToggleMaximize()
            }}
          >
            {isMaximized ? <CloseFullscreenRounded /> : <OpenInFullRounded />}
          </IconButton>
        </ButtonGroup>
      </DialogContent>

      <DialogActions>
        <Button onClick={handleClose} variant="outlined">
          {t('shared.actions.cancel')}
        </Button>
        <Button
          onClick={() => {
            void handleSave()
          }}
          variant="contained"
          disabled={disableSave}
        >
          {t('shared.actions.save')}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
