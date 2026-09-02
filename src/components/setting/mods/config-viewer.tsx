import { Box, Chip, Tab, Tabs } from '@mui/material'
import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import type { DialogRef } from '@/components/base'
import { EditorViewer } from '@/components/profile/editor-viewer'
import { useProfiles } from '@/hooks/use-profiles'
import { getRuntimeYaml, readProfileFile } from '@/services/cmds'

type ConfigSource = 'runtime' | 'provider'

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
  return (
    <EditorViewer
      open={true}
      title={
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 2 }}>
          <Tabs
            value={source}
            onChange={(_event, next: ConfigSource) => {
              setSource(next)
            }}
            sx={{ minHeight: 36, '& .MuiTab-root': { minHeight: 36 } }}
          >
            <Tab
              value="runtime"
              label={t(
                'settings.components.verge.advanced.fields.runtimeConfig',
              )}
            />
            <Tab
              value="provider"
              label={t(
                'settings.components.verge.advanced.fields.providerConfig',
              )}
            />
          </Tabs>
          <Chip label={t('shared.labels.readOnly')} size="small" />
        </Box>
      }
      value={source === 'runtime' ? runtimeConfig : providerText}
      readOnly
      language="yaml"
      path={
        source === 'runtime' ? 'runtime-config.yaml' : 'provider-config.yaml'
      }
      loading={source === 'runtime' ? runtimeLoading : providerLoading}
      onClose={() => setOpen(false)}
    />
  )
})
