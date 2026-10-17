import {
  CloseFullscreenRounded,
  LockRounded,
  OpenInFullRounded,
} from '@mui/icons-material'
import { alpha, Box, IconButton } from '@mui/material'
import { useLockFn } from 'ahooks'
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
import { useWindowControls } from '@/hooks/use-window'
import { getRuntimeYaml, readProfileFile } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import type { MonacoEditorInstance } from '@/types/monaco'
import { errorReason } from '@/utils/error-explanation'

type ConfigSource = 'runtime' | 'provider'

const syncModel = (editor: MonacoEditorInstance | null, value: string) => {
  const model = editor?.getModel()
  if (model && model.getValue() !== value) model.setValue(value)
}

export const ConfigViewer = forwardRef<DialogRef>((_, ref) => {
  const { t } = useTranslation()
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

  const { maximized: isMaximized, toggleMaximize: toggleWindow } =
    useWindowControls()

  const toggleMaximize = useLockFn(async () => {
    try {
      await toggleWindow()
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
      const failed = (reason?: string) =>
        `# ${t('settings.modals.config.runtimeFailed')}${reason ? `: ${reason}` : ''}\n`
      getRuntimeYaml()
        .then((data) => {
          setRuntimeConfig(data ?? failed())
        })
        .catch((err: unknown) => {
          setRuntimeConfig(failed(errorReason(err, t)))
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
            isMaximized ? 'shared.window.restore' : 'shared.window.maximize',
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
            loading={null}
            saveViewState
            keepCurrentModel={false}
            options={{
              automaticLayout: true,
              readOnly: true,
              readOnlyMessage: {
                value: t('profiles.modals.editor.messages.readOnly'),
              },
              renderValidationDecorations: 'on',
              padding: { top: 12 },
            }}
          />
        )}
      </Box>
    </BaseDialog>
  )
})
