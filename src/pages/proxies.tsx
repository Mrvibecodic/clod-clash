import { LanOutlined, LanRounded, WarningRounded } from '@mui/icons-material'
import { Box, Button } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useReducer, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { BasePage, BaseSegmented, TooltipIcon } from '@/components/base'
import { ProviderButton } from '@/components/proxy/provider-button'
import { ProxyGroups } from '@/components/proxy/proxy-groups'
import { useProfiles } from '@/hooks/use-profiles'
import { NARROW } from '@/pages/_theme'
import { useClashConfigData } from '@/providers/app-data-context'
import {
  getRuntimeProxyChainConfig,
  patchClashMode,
  updateProxyChainConfigInRuntime,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
import { readProxyChain } from '@/services/proxy-chain-store'
import { debugLog } from '@/utils/debug'

const MODES = ['rule', 'global', 'direct'] as const
type Mode = (typeof MODES)[number]
const MODE_SET = new Set<string>(MODES)
const isMode = (value: unknown): value is Mode =>
  typeof value === 'string' && MODE_SET.has(value)

const ProxyPage = () => {
  const { t } = useTranslation()

  // Восстанавливаем состояние кнопки цепочки прокси из localStorage
  const [isChainMode, setIsChainMode] = useState(() => {
    try {
      const saved = localStorage.getItem('proxy-chain-mode-enabled')
      return saved === 'true'
    } catch {
      return false
    }
  })

  const [chainConfigData, dispatchChainConfigData] = useReducer(
    (_: string | null, action: string | null) => action,
    null as string | null,
  )

  const { clashConfig } = useClashConfigData()

  const updateChainConfigData = useCallback((value: string | null) => {
    dispatchChainConfigData(value)
  }, [])
  const normalizedMode = clashConfig?.mode?.toLowerCase()
  const curMode = isMode(normalizedMode) ? normalizedMode : undefined
  // clod: `clod-lock-mode` hides every mode switch, this page included —
  // the settings page alone would leave the lock trivially bypassable.
  const { current } = useProfiles()
  const modeLocked = Boolean(current?.lock_mode)
  const chainWarning = t('proxies.page.chain.warning')

  const onChangeMode = useLockFn(async (mode: Mode) => {
    // clod:Э11-10 — разрыв соединений здесь больше не дублируется. Его делает
    // бэкенд по той же настройке `auto_close_connection`, и делает на ВСЕХ путях
    // смены режима: режим меняют ещё трей и горячие клавиши, мимо этой страницы.
    try {
      // patchClashMode отклоняется, если PATCH на бэкенде не удался — нужно уведомить
      // пользователя, а не проглатывать ошибку молча
      // Режим на экране перечитывает событие обновления конфига от бэкенда.
      await patchClashMode(mode)
    } catch (error) {
      showNotice.error(error)
    }
  })

  const onToggleChainMode = useLockFn(async () => {
    const newChainMode = !isChainMode

    setIsChainMode(newChainMode)
    // Сохраняем состояние кнопки цепочки прокси в localStorage
    localStorage.setItem('proxy-chain-mode-enabled', newChainMode.toString())

    if (!newChainMode) {
      // При выходе из режима цепочки прокси очищаем конфигурацию цепочки
      try {
        debugLog('Exiting chain mode, clearing chain configuration')
        await updateProxyChainConfigInRuntime(null)
        debugLog('Chain configuration cleared successfully')
      } catch (error) {
        console.error('Failed to clear chain configuration:', error)
      }
    }
  })

  // При включении режима цепочки прокси получаем данные конфигурации
  useEffect(() => {
    if (!isChainMode) {
      updateChainConfigData(null)
      return
    }

    let cancelled = false

    const fetchChainConfig = async () => {
      try {
        const exitNode = readProxyChain(current?.uid).exitNode

        if (!exitNode) {
          if (!cancelled) {
            updateChainConfigData('')
          }
          return
        }

        const configData = await getRuntimeProxyChainConfig(exitNode)
        if (!cancelled) {
          updateChainConfigData(configData || '')
        }
      } catch (error) {
        console.error('Failed to get runtime proxy chain config:', error)
        if (!cancelled) {
          updateChainConfigData('')
        }
      }
    }

    fetchChainConfig()

    return () => {
      cancelled = true
    }
  }, [isChainMode, current?.uid, updateChainConfigData])

  useEffect(() => {
    if (normalizedMode && !isMode(normalizedMode)) {
      onChangeMode('rule')
    }
  }, [normalizedMode, onChangeMode])

  return (
    <BasePage
      full
      contentStyle={{ height: '100%' }}
      title={
        isChainMode ? (
          <Box
            component="span"
            data-tauri-drag-region="true"
            sx={{
              display: 'inline-flex',
              alignItems: 'center',
              gap: 0.75,
              maxWidth: '100%',
              verticalAlign: 'middle',
            }}
          >
            <Box
              component="span"
              sx={{ minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis' }}
            >
              {t('proxies.page.title.chainMode')}
            </Box>
            <TooltipIcon
              title={chainWarning}
              icon={WarningRounded}
              color="warning"
              sx={{ p: 0.25 }}
            />
          </Box>
        ) : (
          t('proxies.page.title.default')
        )
      }
      header={
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          <ProviderButton />

          {!modeLocked && (
            <BaseSegmented<Mode | ''>
              value={curMode ?? ''}
              options={MODES.map((mode) => ({
                value: mode,
                label: t(`proxies.page.modes.${mode}`),
              }))}
              onChange={(mode) => {
                if (mode) onChangeMode(mode)
              }}
              sx={{
                [NARROW]: {
                  '& .MuiToggleButton-root': { px: 1, fontSize: 12.5 },
                },
              }}
            />
          )}

          <Button
            size="small"
            variant={isChainMode ? 'contained' : 'outlined'}
            onClick={onToggleChainMode}
            title={t('proxies.page.actions.toggleChain')}
            sx={{
              ml: 1,
              whiteSpace: 'nowrap',
              [NARROW]: {
                minWidth: 0,
                '& .chain-label': { display: 'none' },
                '& .MuiButton-startIcon': { m: 0 },
              },
            }}
            startIcon={
              isChainMode ? (
                <LanRounded fontSize="small" />
              ) : (
                <LanOutlined fontSize="small" />
              )
            }
          >
            <span className="chain-label">
              {t('proxies.page.actions.toggleChain')}
            </span>
          </Button>
        </Box>
      }
    >
      <ProxyGroups
        mode={curMode ?? 'rule'}
        isChainMode={isChainMode}
        chainConfigData={chainConfigData}
      />
    </BasePage>
  )
}

export default ProxyPage
