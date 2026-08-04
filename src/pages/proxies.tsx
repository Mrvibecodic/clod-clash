import { LanOutlined, LanRounded, WarningRounded } from '@mui/icons-material'
import { Box, Button, ButtonGroup } from '@mui/material'
import { useLockFn } from 'ahooks'
import { useCallback, useEffect, useReducer, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { closeAllConnections } from 'tauri-plugin-mihomo-api'

import { BasePage, TooltipIcon } from '@/components/base'
import { ProviderButton } from '@/components/proxy/provider-button'
import { ProxyGroups } from '@/components/proxy/proxy-groups'
import { useGroupTestUrls } from '@/hooks/use-group-test-urls'
import { useProfiles } from '@/hooks/use-profiles'
import { useVerge } from '@/hooks/use-verge'
import {
  useAppRefreshers,
  useClashConfigData,
} from '@/providers/app-data-context'
import {
  getRuntimeProxyChainConfig,
  patchClashMode,
  updateProxyChainConfigInRuntime,
} from '@/services/cmds'
import { showNotice } from '@/services/notice-service'
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
  const { refreshClashConfig } = useAppRefreshers()

  const updateChainConfigData = useCallback((value: string | null) => {
    dispatchChainConfigData(value)
  }, [])
  const { verge } = useVerge()
  // clod: наполняет delayManager адресами `url:` групп и общим дефолтом. Без
  // этого страница «Прокси» видела бы их только после захода на главную —
  // раньше пробел закрывали записи в urlMap, но они переживали смену профиля.
  useGroupTestUrls()

  const normalizedMode = clashConfig?.mode?.toLowerCase()
  const curMode = isMode(normalizedMode) ? normalizedMode : undefined
  // clod: `clod-lock-mode` hides every mode switch, this page included —
  // the settings page alone would leave the lock trivially bypassable.
  const { current } = useProfiles()
  const modeLocked = Boolean(current?.lock_mode)
  const chainWarning = t('proxies.page.chain.warning')

  const onChangeMode = useLockFn(async (mode: Mode) => {
    // Разрываем соединение
    if (mode !== curMode && verge?.auto_close_connection) {
      closeAllConnections()
    }
    try {
      // patchClashMode отклоняется, если PATCH на бэкенде не удался — нужно уведомить
      // пользователя, а не проглатывать ошибку молча
      await patchClashMode(mode)
      refreshClashConfig()
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
        const exitNode = localStorage.getItem('proxy-chain-exit-node')

        if (!exitNode) {
          console.error('No proxy chain exit node found in localStorage')
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
  }, [isChainMode, updateChainConfigData])

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
            sx={{ display: 'inline-flex', alignItems: 'center', gap: 0.75 }}
          >
            {t('proxies.page.title.chainMode')}
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
            <ButtonGroup size="small">
              {MODES.map((mode) => (
                <Button
                  key={mode}
                  variant={mode === curMode ? 'contained' : 'outlined'}
                  onClick={() => onChangeMode(mode)}
                  sx={{ textTransform: 'capitalize' }}
                >
                  {t(`proxies.page.modes.${mode}`)}
                </Button>
              ))}
            </ButtonGroup>
          )}

          <Button
            size="small"
            variant={isChainMode ? 'contained' : 'outlined'}
            onClick={onToggleChainMode}
            sx={{ ml: 1 }}
            startIcon={
              isChainMode ? (
                <LanRounded fontSize="small" />
              ) : (
                <LanOutlined fontSize="small" />
              )
            }
          >
            {t('proxies.page.actions.toggleChain')}
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
