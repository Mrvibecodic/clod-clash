import { listen } from '@tauri-apps/api/event'
import React, { useCallback, useEffect, useMemo, useRef } from 'react'
import {
  getBaseConfig,
  getRuleProviders,
  getRules,
} from 'tauri-plugin-mihomo-api'

import { useVerge } from '@/hooks/use-verge'
import {
  calcuProxies,
  calcuProxyProviders,
  getRunningMode,
  getSystemProxy,
} from '@/services/cmds'
import { revalidateQueries, useQuery } from '@/services/query-client'

import {
  ClashConfigContext,
  ProxiesContext,
  RefreshersContext,
  RulesContext,
  SystemContext,
} from './app-data-context'

const TQ_MIHOMO = {
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  staleTime: 1500,
  retry: 3,
  retryDelay: (attempt: number) => Math.min(200 * 2 ** attempt, 3000),
} as const

const TQ_DEFAULTS = {
  refetchOnWindowFocus: false,
  refetchOnReconnect: false,
  staleTime: 5000,
  retry: 2,
} as const

function useStableFn<T extends (...args: any[]) => any>(fn: T): T {
  const ref = useRef(fn)
  ref.current = fn
  return useCallback((...args: Parameters<T>) => ref.current(...args), []) as T
}

// Компонент глобального провайдера данных
export const AppDataProvider = ({
  children,
}: {
  children: React.ReactNode
}) => {
  const { verge } = useVerge()

  const {
    data: proxiesData,
    isPending: isProxiesPending,
    refetch: _refetchProxy,
  } = useQuery({
    queryKey: ['getProxies'],
    queryFn: calcuProxies,
    ...TQ_MIHOMO,
  })

  const {
    data: clashConfig,
    isPending: isClashConfigPending,
    refetch: _refetchClashConfig,
  } = useQuery({
    queryKey: ['getClashConfig'],
    queryFn: getBaseConfig,
    ...TQ_MIHOMO,
  })

  const { data: proxyProviders, refetch: _refetchProxyProviders } = useQuery({
    queryKey: ['getProxyProviders'],
    queryFn: calcuProxyProviders,
    ...TQ_MIHOMO,
    revalidateOnMount: false,
  })

  const { data: ruleProviders, refetch: _refetchRuleProviders } = useQuery({
    queryKey: ['getRuleProviders'],
    queryFn: getRuleProviders,
    ...TQ_MIHOMO,
    revalidateOnMount: false,
  })

  const { data: rulesData, refetch: _refetchRules } = useQuery({
    queryKey: ['getRules'],
    queryFn: getRules,
    ...TQ_MIHOMO,
  })

  const { data: sysproxy, refetch: _refetchSysproxy } = useQuery({
    queryKey: ['getSystemProxy'],
    queryFn: getSystemProxy,
    ...TQ_DEFAULTS,
  })

  const { data: runningMode } = useQuery({
    queryKey: ['getRunningMode'],
    queryFn: getRunningMode,
    ...TQ_DEFAULTS,
  })

  const refreshProxy = useStableFn(_refetchProxy)
  const refreshClashConfig = useStableFn(_refetchClashConfig)
  const refreshRules = useStableFn(_refetchRules)
  const refreshSysproxy = useStableFn(_refetchSysproxy)
  const refreshProxyProviders = useStableFn(_refetchProxyProviders)
  const refreshRuleProviders = useStableFn(_refetchRuleProviders)

  useEffect(() => {
    let lastProfileId: string | null = null
    let lastProfileUpdateTime = 0
    let lastProxyUpdateTime = 0
    const refreshThrottle = 800
    const cleanupFns: Array<() => void> = []

    const handleProfileChanged = (event: { payload: string }) => {
      const newProfileId = event.payload
      const now = Date.now()
      if (
        lastProfileId === newProfileId &&
        now - lastProfileUpdateTime < refreshThrottle
      ) {
        return
      }
      lastProfileId = newProfileId
      lastProfileUpdateTime = now
      void revalidateQueries([['getProfiles']])
    }

    const handleRefreshProxy = () => {
      const now = Date.now()
      if (now - lastProxyUpdateTime <= refreshThrottle) return
      lastProxyUpdateTime = now
      refreshProxy().catch(() => {})
    }

    const handleRefreshProfiles = () => {
      void revalidateQueries([['getProfiles']])
    }

    const initializeListeners = async () => {
      try {
        const unlistenProfile = await listen<string>(
          'profile-changed',
          handleProfileChanged,
        )
        cleanupFns.push(unlistenProfile)
      } catch (error) {
        console.error(
          '[AppDataProvider] Не удалось подписаться на событие Profile:',
          error,
        )
      }

      try {
        const unlistenProfiles = await listen(
          'verge://refresh-profiles',
          handleRefreshProfiles,
        )
        cleanupFns.push(unlistenProfiles)
      } catch (error) {
        console.error(
          '[AppDataProvider] Не удалось подписаться на событие обновления Profiles:',
          error,
        )
      }

      try {
        const unlistenProxy = await listen(
          'verge://refresh-proxy-config',
          handleRefreshProxy,
        )
        cleanupFns.push(unlistenProxy)
      } catch (error) {
        console.warn(
          '[AppDataProvider] Не удалось установить слушатель событий Tauri:',
          error,
        )
      }
    }

    void initializeListeners()

    return () => {
      cleanupFns.forEach((fn) => {
        try {
          fn()
        } catch (error) {
          console.error('[DataProvider] Cleanup error:', error)
        }
      })
    }
  }, [refreshProxy])

  const refreshAll = useCallback(async () => {
    await Promise.all([
      refreshProxy(),
      refreshClashConfig(),
      refreshRules(),
      refreshSysproxy(),
      refreshProxyProviders(),
      refreshRuleProviders(),
    ])
  }, [
    refreshProxy,
    refreshClashConfig,
    refreshRules,
    refreshSysproxy,
    refreshProxyProviders,
    refreshRuleProviders,
  ])

  const proxiesValue = useMemo(
    () => ({
      proxies: proxiesData,
      proxyProviders: proxyProviders || {},
      isProxiesPending,
    }),
    [proxiesData, proxyProviders, isProxiesPending],
  )

  const rulesValue = useMemo(
    () => ({
      rules: rulesData?.rules ?? [],
      ruleProviders: ruleProviders?.providers || {},
    }),
    [rulesData, ruleProviders],
  )

  const clashConfigValue = useMemo(
    () => ({
      clashConfig,
      isClashConfigPending,
    }),
    [clashConfig, isClashConfigPending],
  )

  const systemValue = useMemo(() => {
    const calculateSystemProxyAddress = () => {
      if (!verge || !clashConfig) return '-'

      const isPacMode = verge.proxy_auto_config ?? false

      if (isPacMode) {
        // Режим PAC: показываем адрес прокси, который мы ожидаем установить
        const proxyHost = verge.proxy_host || '127.0.0.1'
        const proxyPort =
          verge.verge_mixed_port || clashConfig.mixedPort || 7897
        return `${proxyHost}:${proxyPort}`
      } else {
        // Режим HTTP-прокси: предпочитаем системный адрес, но если формат
        // некорректен, используем ожидаемый адрес
        const systemServer = sysproxy?.server
        if (
          systemServer &&
          systemServer !== '-' &&
          !systemServer.startsWith(':')
        ) {
          return systemServer
        } else {
          // Системный адрес недействителен, возвращаем ожидаемый адрес прокси
          const proxyHost = verge.proxy_host || '127.0.0.1'
          const proxyPort =
            verge.verge_mixed_port || clashConfig.mixedPort || 7897
          return `${proxyHost}:${proxyPort}`
        }
      }
    }

    return {
      sysproxy,
      runningMode,
      systemProxyAddress: calculateSystemProxyAddress(),
    }
  }, [sysproxy, runningMode, verge, clashConfig])

  const refreshersValue = useMemo(
    () => ({
      refreshProxy,
      refreshClashConfig,
      refreshRules,
      refreshSysproxy,
      refreshProxyProviders,
      refreshRuleProviders,
      refreshAll,
    }),
    [
      refreshProxy,
      refreshClashConfig,
      refreshRules,
      refreshSysproxy,
      refreshProxyProviders,
      refreshRuleProviders,
      refreshAll,
    ],
  )

  return (
    <ProxiesContext value={proxiesValue}>
      <RulesContext value={rulesValue}>
        <ClashConfigContext value={clashConfigValue}>
          <SystemContext value={systemValue}>
            <RefreshersContext value={refreshersValue}>
              {children}
            </RefreshersContext>
          </SystemContext>
        </ClashConfigContext>
      </RulesContext>
    </ProxiesContext>
  )
}
