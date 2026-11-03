import { RefreshRounded } from '@mui/icons-material'
import {
  Box,
  Button,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Divider,
  IconButton,
  List,
  ListItem,
  ListItemText,
  Typography,
  alpha,
} from '@mui/material'
import { useLockFn } from 'ahooks'
import dayjs from 'dayjs'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { showNotice } from '@/services/notice-service'
import type { TranslationKey } from '@/types/generated/i18n-keys'

interface Props<P extends { updatedAt?: string | null }> {
  open: boolean
  onClose: () => void
  title: string
  providers: Record<string, P> | undefined
  updateOne: (name: string) => Promise<unknown>
  /** Перечитать провайдеров после обновления. */
  refresh: () => Promise<unknown>
  noneKey: TranslationKey
  allUpdatedKey: TranslationKey
  /** Плашки справа от имени. */
  badges: (provider: P) => ReactNode
  /** Строка под именем. */
  details: (provider: P) => ReactNode
}

/** Окно «Наборы узлов» / «Наборы правил»: список провайдеров ядра и их обновление. */
export function ProviderDialog<P extends { updatedAt?: string | null }>({
  open,
  onClose,
  title,
  providers,
  updateOne,
  refresh,
  noneKey,
  allUpdatedKey,
  badges,
  details,
}: Props<P>) {
  const { t } = useTranslation()
  const [updating, setUpdating] = useState<Record<string, boolean>>({})

  const updateProvider = async (name: string) => {
    try {
      setUpdating((prev) => ({ ...prev, [name]: true }))
      await updateOne(name)
      await refresh()
      showNotice.success(
        'shared.feedback.notifications.provider.updateSuccess',
        {
          name,
        },
      )
    } catch (err) {
      showNotice.error('shared.feedback.notifications.provider.updateFailed', {
        name,
        message: String(err),
      })
    } finally {
      setUpdating((prev) => ({ ...prev, [name]: false }))
    }
  }

  const updateAllProviders = useLockFn(async () => {
    try {
      const allProviders = Object.keys(providers || {})
      if (allProviders.length === 0) {
        showNotice.info(noneKey)
        return
      }

      setUpdating(Object.fromEntries(allProviders.map((name) => [name, true])))

      const failures: { name: string; error: unknown }[] = []
      // По одному: ядро обновляет провайдеров последовательно
      for (const name of allProviders) {
        try {
          await updateOne(name)
        } catch (error) {
          failures.push({ name, error })
        } finally {
          setUpdating((prev) => ({ ...prev, [name]: false }))
        }
      }

      await refresh()

      if (failures.length === 0) {
        showNotice.success(allUpdatedKey)
      } else {
        showNotice.error(
          'shared.feedback.notifications.provider.updateFailed',
          {
            name: failures.map((failure) => failure.name).join(', '),
            message: String(failures[0].error),
          },
        )
      }
    } catch (err) {
      showNotice.error('shared.feedback.notifications.provider.genericError', {
        message: String(err),
      })
    } finally {
      setUpdating({})
    }
  })

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth>
      <DialogTitle>
        <Box
          sx={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
          }}
        >
          <Typography variant="h6">{title}</Typography>
          <Button variant="contained" size="small" onClick={updateAllProviders}>
            {t('shared.actions.updateAll')}
          </Button>
        </Box>
      </DialogTitle>

      <DialogContent>
        <List sx={{ py: 0, minHeight: 250 }}>
          {Object.entries(providers || {})
            .sort()
            .map(([key, provider]) => {
              const isUpdating = updating[key]
              return (
                <ListItem
                  key={key}
                  sx={[
                    {
                      p: 0,
                      mb: '8px',
                      borderRadius: 2,
                      overflow: 'hidden',
                      transition: 'all 0.2s',
                    },
                    ({ palette: { mode, primary, background } }) => ({
                      backgroundColor: background.paper,
                      '&:hover': {
                        backgroundColor:
                          mode === 'light'
                            ? alpha(primary.main, 0.1)
                            : alpha(primary.main, 0.2),
                      },
                    }),
                  ]}
                >
                  <ListItemText
                    sx={{ px: 2, py: 1 }}
                    primary={
                      <Box
                        sx={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'center',
                        }}
                      >
                        <Typography
                          variant="subtitle1"
                          component="div"
                          noWrap
                          title={key}
                          sx={{ display: 'flex', alignItems: 'center' }}
                        >
                          <span style={{ marginRight: '8px' }}>{key}</span>
                          {badges(provider)}
                        </Typography>

                        <Typography
                          variant="body2"
                          color="text.secondary"
                          noWrap
                        >
                          <small>{t('shared.labels.updateAt')}: </small>
                          {dayjs(provider.updatedAt).fromNow()}
                        </Typography>
                      </Box>
                    }
                    secondary={details(provider)}
                  />
                  <Divider orientation="vertical" flexItem />
                  <Box
                    sx={{
                      width: 40,
                      display: 'flex',
                      justifyContent: 'center',
                      alignItems: 'center',
                    }}
                  >
                    <IconButton
                      size="small"
                      color="primary"
                      onClick={() => void updateProvider(key)}
                      disabled={isUpdating}
                      sx={{
                        animation: isUpdating
                          ? 'spin 1s linear infinite'
                          : 'none',
                        '@keyframes spin': {
                          '0%': { transform: 'rotate(0deg)' },
                          '100%': { transform: 'rotate(360deg)' },
                        },
                      }}
                      title={t('shared.actions.update')}
                      aria-label={t('shared.actions.update')}
                    >
                      <RefreshRounded />
                    </IconButton>
                  </Box>
                </ListItem>
              )
            })}
        </List>
      </DialogContent>

      <DialogActions>
        <Button onClick={onClose} variant="outlined">
          {t('shared.actions.close')}
        </Button>
      </DialogActions>
    </Dialog>
  )
}
