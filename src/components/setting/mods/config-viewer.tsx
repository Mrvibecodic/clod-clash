import {
  CloseFullscreenRounded,
  LockRounded,
  OpenInFullRounded,
} from '@mui/icons-material'
import { alpha, Box, IconButton } from '@mui/material'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { useLockFn } from 'ahooks'
import { debounce } from 'lodash-es'
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseDialog,
  BaseLoadingOverlay,
  BaseSegmented,
  type DialogRef,
  MonacoEditor,
} from '@/components/base'
import { useProfiles } from '@/hooks/use-profiles'
import { getRuntimeYaml, readProfileFile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { useThemeMode } from '@/services/states'
import type { MonacoEditorInstance } from '@/types/monaco'
import getSystem from '@/utils/get-system'

type ConfigSource = 'runtime' | 'provider'

const appWindow = getCurrentWebviewWindow()

const syncModel = (editor: MonacoEditorInstance | null, value: string) => {
  const model = editor?.getModel()
  if (model && model.getValue() !== value) model.setValue(value)
}

const EDITOR_FONT = `Fira Code, JetBrains Mono, Roboto Mono, "Source Code Pro", Consolas, Menlo, Monaco, monospace, "Courier New", "Apple Color Emoji"${
  getSystem() === 'windows' ? ', twemoji mozilla' : ''
}`

export const ConfigViewer = forwardRef<DialogRef>((_, ref) => {
  const { t } = useTranslation()
  const themeMode = useThemeMode()
  const { current } = useProfiles()
  const uid = current?.uid
  const [open, setOpen] = useState(false)
  const [runtimeLoading, setRuntimeLoading] = useState(false)
  const [providerLoading, setProviderLoading] = useState(false)
  const [source, setSource] = useState<ConfigSource>('runtime')
  const [runtimeConfig, setRuntimeConfig] = useState('')
  const [providerConfig, setProviderConfig] = useState<{
    uid?: string
    text: string
  }>({ text: '' })

  const providerText = providerConfig.uid === uid ? providerConfig.text : ''

  const [isMaximized, setIsMaximized] = useState(false)

  const syncMaximized = useCallback(async () => {
    try {
      setIsMaximized(await appWindow.isMaximized())
    } catch {
      setIsMaximized(false)
    }
  }, [])

  useEffect(() => {
    if (!open) return
    void syncMaximized()
    const onResized = debounce(() => void syncMaximized(), 100)
    const unlisten = appWindow.onResized(onResized)
    return () => {
      onResized.cancel()
      unlisten.then((fn) => fn())
    }
  }, [open, syncMaximized])

  const toggleMaximize = useLockFn(async () => {
    try {
      await appWindow.toggleMaximize()
      await syncMaximized()
    } catch (error) {
      showNotice.error(error)
    }
  })

  const editorRef = useRef<MonacoEditorInstance | null>(null)
  const value = source === 'runtime' ? runtimeConfig : providerText
  const path =
    source === 'runtime' ? 'runtime-config.yaml' : 'provider-config.yaml'

  // Существующую модель по path редактор берёт как есть, без value.
  useEffect(() => {
    syncModel(editorRef.current, value)
  }, [value, path])

  const uidRef = useRef(uid)
  uidRef.current = uid

  const loadProviderConfig = useCallback(async () => {
    const failed = `# ${t('settings.components.verge.advanced.messages.profileFileError')}\n`
    if (!uid) {
      setProviderConfig({
        text: `# ${t('settings.components.verge.advanced.messages.noProfileSelected')}\n`,
      })
      return
    }
    setProviderLoading(true)
    try {
      const data = await readProfileFile(uid)
      if (uidRef.current !== uid) return
      setProviderConfig({ uid, text: data || failed })
    } catch {
      if (uidRef.current !== uid) return
      setProviderConfig({ uid, text: failed })
    } finally {
      if (uidRef.current === uid) setProviderLoading(false)
    }
  }, [uid, t])

  useEffect(() => {
    if (!open || source !== 'provider' || providerText) return
    void loadProviderConfig()
  }, [open, source, providerText, loadProviderConfig])

  useImperativeHandle(ref, () => ({
    open: () => {
      setRuntimeConfig('')
      setProviderConfig({ text: '' })
      setSource('runtime')
      setRuntimeLoading(true)
      setProviderLoading(false)
      setOpen(true)
      getRuntimeYaml()
        .then((data) => {
          setRuntimeConfig(data ?? '# Error getting runtime yaml\n')
        })
        .catch(() => {
          setRuntimeConfig('# Error getting runtime yaml\n')
        })
        .finally(() => {
          setRuntimeLoading(false)
        })
    },
    close: () => setOpen(false),
  }))

  if (!open) return null
  const loading = source === 'runtime' ? runtimeLoading : providerLoading
  return (
    <BaseDialog
      open
      title={
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: 1.25,
          }}
        >
          <BaseSegmented
            value={source}
            options={[
              {
                value: 'runtime',
                label: t(
                  'settings.components.verge.advanced.fields.runtimeConfig',
                ),
              },
              {
                value: 'provider',
                label: t(
                  'settings.components.verge.advanced.fields.providerConfig',
                ),
              },
            ]}
            onChange={setSource}
          />
          <Box
            component="span"
            sx={({ palette }) => ({
              display: 'inline-flex',
              alignItems: 'center',
              gap: 0.5,
              px: 1,
              py: 0.375,
              borderRadius: 999,
              bgcolor: alpha(palette.text.primary, 0.08),
              color: 'text.secondary',
              fontSize: 12,
              fontWeight: 600,
            })}
          >
            <LockRounded sx={{ fontSize: 13 }} />
            {t('shared.labels.readOnly')}
          </Box>
        </Box>
      }
      dividers
      fullWidth
      maxWidth="xl"
      disableEnforceFocus
      contentSx={{
        height: 'calc(100vh - 185px)',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'hidden',
      }}
      footerStart={
        <IconButton
          title={t(
            isMaximized ? 'shared.window.minimize' : 'shared.window.maximize',
          )}
          sx={{ ml: -1, color: 'text.secondary' }}
          onClick={() => void toggleMaximize()}
        >
          {isMaximized ? (
            <CloseFullscreenRounded fontSize="small" />
          ) : (
            <OpenInFullRounded fontSize="small" />
          )}
        </IconButton>
      }
      disableOk
      cancelBtn={t('shared.actions.close')}
      onCancel={() => setOpen(false)}
      onClose={() => setOpen(false)}
    >
      <Box
        sx={{
          position: 'relative',
          flex: '1 1 auto',
          minHeight: 0,
          my: 1,
          border: 1,
          borderColor: 'divider',
          borderRadius: '10px',
          overflow: 'hidden',
        }}
      >
        <BaseLoadingOverlay isLoading={loading} />
        {!loading && (
          <MonacoEditor
            height="100%"
            path={path}
            value={value}
            onMount={(editor) => {
              editorRef.current = editor
              syncModel(editor, value)
            }}
            language="yaml"
            theme={themeMode === 'light' ? 'light' : 'vs-dark'}
            loading={null}
            saveViewState
            keepCurrentModel={false}
            options={{
              automaticLayout: true,
              tabSize: 2,
              minimap: {
                enabled: document.documentElement.clientWidth >= 1500,
              },
              mouseWheelZoom: true,
              readOnly: true,
              readOnlyMessage: {
                value: t('profiles.modals.editor.messages.readOnly'),
              },
              renderValidationDecorations: 'on',
              padding: { top: 12 },
              fontFamily: EDITOR_FONT,
              fontLigatures: false,
              smoothScrolling: true,
            }}
          />
        )}
      </Box>
    </BaseDialog>
  )
})
