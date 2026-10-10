import Visibility from '@mui/icons-material/Visibility'
import VisibilityOff from '@mui/icons-material/VisibilityOff'
import {
  Box,
  Button,
  IconButton,
  InputAdornment,
  TextField,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import { useState, useRef, memo, useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { useTranslation } from 'react-i18next'

import { FormField } from '@/components/base'
import { MONO_INPUT } from '@/components/base/base-mono'
import { useVerge } from '@/hooks/use-verge'
import { saveWebdavConfig, createWebdavBackup } from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import {
  buildWebdavSignature,
  getWebdavStatus,
  setWebdavStatus,
} from '@/services/webdav-status'
import { isValidUrl } from '@/utils/network'

const TEXT_PROPS = {
  fullWidth: true,
  size: 'small',
  autoCorrect: 'off',
  autoCapitalize: 'off',
  spellCheck: 'false',
} as const

interface BackupConfigViewerProps {
  onBackupSuccess: () => Promise<void>
  onRefresh: () => Promise<void>
  onInit: () => Promise<void>
  setLoading: (loading: boolean) => void
}

export const BackupConfigViewer = memo(
  ({
    onBackupSuccess,
    onRefresh,
    onInit,
    setLoading,
  }: BackupConfigViewerProps) => {
    const { t } = useTranslation()
    const { verge, mutateVerge } = useVerge()
    const { webdav_url, webdav_username, webdav_password } = verge || {}
    const [showPassword, setShowPassword] = useState(false)
    const usernameRef = useRef<HTMLInputElement>(null)
    const passwordRef = useRef<HTMLInputElement>(null)
    const urlRef = useRef<HTMLInputElement>(null)

    const { register, handleSubmit, watch } = useForm<IWebDavConfig>({
      defaultValues: {
        url: webdav_url,
        username: webdav_username,
        password: webdav_password,
      },
    })
    const url = watch('url')
    const username = watch('username')
    const password = watch('password')

    const webdavChanged =
      webdav_url !== url ||
      webdav_username !== username ||
      webdav_password !== password

    const webdavSignature = buildWebdavSignature(verge)
    const webdavStatus = getWebdavStatus(webdavSignature)
    const shouldAutoInit = webdavStatus !== 'failed'

    const handleClickShowPassword = () => {
      setShowPassword((prev) => !prev)
    }

    useEffect(() => {
      if (!webdav_url || !webdav_username || !webdav_password) {
        return
      }
      if (!shouldAutoInit) {
        return
      }
      void onInit()
    }, [webdav_url, webdav_username, webdav_password, onInit, shouldAutoInit])

    const checkForm = () => {
      const username = usernameRef.current?.value
      const password = passwordRef.current?.value
      const url = urlRef.current?.value

      if (!url) {
        urlRef.current?.focus()
        showNotice.error('settings.modals.backup.messages.webdavUrlRequired')
        throw new Error(t('settings.modals.backup.messages.webdavUrlRequired'))
      } else if (!isValidUrl(url)) {
        urlRef.current?.focus()
        showNotice.error('settings.modals.backup.messages.invalidWebdavUrl')
        throw new Error(t('settings.modals.backup.messages.invalidWebdavUrl'))
      }
      if (!username) {
        usernameRef.current?.focus()
        showNotice.error('settings.modals.backup.messages.usernameRequired')
        throw new Error(t('settings.modals.backup.messages.usernameRequired'))
      }
      if (!password) {
        passwordRef.current?.focus()
        showNotice.error('settings.modals.backup.messages.passwordRequired')
        throw new Error(t('settings.modals.backup.messages.passwordRequired'))
      }
    }

    const save = useLockFn(async (data: IWebDavConfig) => {
      checkForm()
      const signature = buildWebdavSignature({
        webdav_url: data.url,
        webdav_username: data.username,
      })
      const trimmedUrl = data.url.trim()
      const trimmedUsername = data.username.trim()

      try {
        setLoading(true)
        await saveWebdavConfig(trimmedUrl, trimmedUsername, data.password)
        // До обновления настроек: в подписи нет пароля, и отрисовка с новым
        // паролем не должна застать прежний «не отвечает».
        setWebdavStatus(signature, 'unknown')
        await mutateVerge(
          (current) =>
            current
              ? {
                  ...current,
                  webdav_url: trimmedUrl,
                  webdav_username: trimmedUsername,
                  webdav_password: data.password,
                }
              : current,
          false,
        )
        // Проверку сервера после сохранения делает эффект автоинициализации:
        // новые учётные данные меняют его зависимости.
        showNotice.success('settings.modals.backup.messages.webdavConfigSaved')
      } catch (error) {
        showNotice.error(
          'settings.modals.backup.messages.webdavConfigSaveFailed',
          { error },
          3000,
        )
      } finally {
        setLoading(false)
      }
    })

    const handleBackup = useLockFn(async () => {
      checkForm()
      const signature = buildWebdavSignature({
        webdav_url: url,
        webdav_username: username,
      })

      try {
        setLoading(true)
        await createWebdavBackup().then(async () => {
          showNotice.success('settings.modals.backup.messages.backupCreated')
          await onBackupSuccess()
        })
        setWebdavStatus(signature, 'ready')
      } catch (error) {
        showNotice.error('settings.modals.backup.messages.backupFailed', {
          error,
        })
        setWebdavStatus(signature, 'failed')
      } finally {
        setLoading(false)
      }
    })

    return (
      <form onSubmit={(e) => e.preventDefault()}>
        <FormField label={t('settings.modals.backup.fields.webdavUrl')}>
          <TextField
            {...TEXT_PROPS}
            {...register('url')}
            inputRef={urlRef}
            sx={MONO_INPUT}
          />
        </FormField>
        <Box
          sx={{
            display: 'grid',
            gridTemplateColumns: 'repeat(2, minmax(0, 1fr))',
            columnGap: 1.5,
          }}
        >
          <FormField label={t('settings.modals.backup.fields.username')}>
            <TextField
              {...TEXT_PROPS}
              {...register('username')}
              inputRef={usernameRef}
            />
          </FormField>
          <FormField label={t('shared.labels.password')}>
            <TextField
              {...TEXT_PROPS}
              type={showPassword ? 'text' : 'password'}
              inputRef={passwordRef}
              {...register('password')}
              slotProps={{
                input: {
                  endAdornment: (
                    <InputAdornment position="end">
                      <IconButton
                        size="small"
                        edge="end"
                        sx={{ color: 'text.secondary' }}
                        onClick={handleClickShowPassword}
                      >
                        {showPassword ? (
                          <VisibilityOff fontSize="small" />
                        ) : (
                          <Visibility fontSize="small" />
                        )}
                      </IconButton>
                    </InputAdornment>
                  ),
                },
              }}
            />
          </FormField>
        </Box>
        <Box
          sx={{
            display: 'flex',
            flexWrap: 'wrap',
            justifyContent: 'flex-end',
            gap: 1,
            pt: 0.75,
            pb: 1.25,
          }}
        >
          {webdavChanged ||
          webdav_url === undefined ||
          webdav_username === undefined ||
          webdav_password === undefined ? (
            <Button
              variant="contained"
              type="button"
              onClick={handleSubmit(save)}
            >
              {t('shared.actions.save')}
            </Button>
          ) : (
            <>
              <Button variant="outlined" onClick={onRefresh} type="button">
                {t('settings.modals.backup.actions.checkConnection')}
              </Button>
              <Button variant="contained" onClick={handleBackup} type="button">
                {t('settings.modals.backup.actions.backup')}
              </Button>
            </>
          )}
        </Box>
      </form>
    )
  },
)
