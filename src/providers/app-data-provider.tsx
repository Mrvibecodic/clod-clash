import { listen } from '@tauri-apps/api/event'
import React, { useCallback, useEffect, useMemo, useRef } from 'react'
import {
  getBaseConfig,
  getRuleProviders,
  getRules,
} from 'tauri-plugin-mihomo-api'

import { useRefreshOnReturn } from '@/hooks/use-refresh-on-return'
import { useVerge } from '@/hooks/use-verge'
import {
  calcuProxies,
  calcuProxyProviders,
  getAutotemProxy,
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

/**
 * Стабильная ссылка: хук возврата держит её в зависимостях эффекта, а ключи
 * те же, по которым ниже подписаны запросы. Через `revalidateQueries`, а не
 * через `refetch` конкретного запроса: тогда порядок объявлений неважен —
 * видимость нужна раньше, чем существуют сами запросы.
 */
const refreshOnReturn = () =>
  revalidateQueries([
    ['getProxies'],
    ['getClashConfig'],
    ['getSystemProxy'],
    ['getAutotemProxy'],
    // Профили сюда же: у них нет ни опроса, ни обновления по фокусу
    // (`use-profiles`), а карточка подписки живёт именно на них — окно сутки в
    // трее означало остаток трафика и срок суточной давности. Запрос дешёвый:
    // `get_profiles` отдаёт уже разобранный конфиг профилей из памяти, в сеть
    // за ним не ходит (за обновление подписки отвечает таймер бэкенда).
    ['getProfiles'],
  ])

/**
 * Как часто перечитываем факт системного прокси в ОС.
 *
 * Не чаще: на Windows это чтение реестра, а на macOS и Linux — порождённые
 * процессы (`networksetup`, `gsettings`). Десять секунд — предел, за которым
 * кнопка ещё честна, а фоновой работы уже не видно.
 */
const SYS_PROXY_POLL_MS = 10_000

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

  // clod: окно вернулось из трея — общие данные ядра перечитываем сразу.
  // Здесь их читает ВСЁ приложение, и на главной у них нет ни опроса, ни
  // обновления по событию: задержки серверов, выбранный узел и состояние
  // системного прокси оставались там от прошлого показа, сколько бы окно ни
  // пролежало свёрнутым. Один запрос на возврат — и экран показывает то, что
  // ядро знает сейчас.
  const visible = useRefreshOnReturn(refreshOnReturn)

  const { data: proxiesData, refetch: _refetchProxy } = useQuery({
    queryKey: ['getProxies'],
    queryFn: calcuProxies,
    ...TQ_MIHOMO,
  })

  const { data: clashConfig, refetch: _refetchClashConfig } = useQuery({
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

  // clod: кнопка Connect горит по ФАКТУ системного прокси в ОС, а не по флагу
  // в нашем конфиге, — и этот факт надо перечитывать. Прокси снимает не только
  // приложение: его гасит другой VPN-клиент, чистилка реестра, падение ядра.
  // Без опроса кнопка оставалась зелёной над выключенным прокси до следующего
  // события от бэкенда, то есть сколько угодно долго. Чтение локальное (реестр
  // на Windows, `networksetup`-снимок на macOS), ядро не трогает.
  const { data: sysproxy } = useQuery({
    queryKey: ['getSystemProxy'],
    queryFn: getSystemProxy,
    ...TQ_DEFAULTS,
    refetchInterval: visible ? SYS_PROXY_POLL_MS : false,
    refetchIntervalInBackground: false,
  })

  // PAC-режим читает другой ключ: там факт — это адрес автонастройки, а не
  // `server` системного прокси (см. `use-system-proxy-state`). Опрос заводим
  // здесь, в единственном экземпляре: сам хук живёт в нескольких местах сразу,
  // и интервал в нём означал бы столько же параллельных опросов.
  useQuery({
    queryKey: ['getAutotemProxy'],
    queryFn: getAutotemProxy,
    ...TQ_DEFAULTS,
    refetchInterval: visible ? SYS_PROXY_POLL_MS : false,
    refetchIntervalInBackground: false,
  })

  const refreshProxy = useStableFn(_refetchProxy)
  const refreshClashConfig = useStableFn(_refetchClashConfig)
  const refreshRules = useStableFn(_refetchRules)
  const refreshProxyProviders = useStableFn(_refetchProxyProviders)
  const refreshRuleProviders = useStableFn(_refetchRuleProviders)

  useEffect(() => {
    let lastProfileId: string | null = null
    let lastProfileUpdateTime = 0
    let lastProxyUpdateTime = 0
    const refreshThrottle = 800
    const cleanupFns: Array<() => void> = []
    // clod:listener-race — `listen()` асинхронный, а очистка эффекта — нет.
    // Размонтирование (или перезапуск эффекта — он зависит от `refreshProxy`)
    // между вызовом и его разрешением заставало массив пустым: подписка
    // доезжала уже ПОСЛЕ уборки, снять её было некому, и она жила до
    // перезагрузки окна, дёргая обновления от лица провайдера, которого больше
    // нет. У быстрой смены профиля это давало по лишней подписке за раз.
    let cancelled = false
    const keep = (unlisten: () => void) => {
      if (cancelled) unlisten()
      else cleanupFns.push(unlisten)
    }

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
        keep(unlistenProfile)
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
        keep(unlistenProfiles)
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
        keep(unlistenProxy)
      } catch (error) {
        console.warn(
          '[AppDataProvider] Не удалось установить слушатель событий Tauri:',
          error,
        )
      }
    }

    void initializeListeners()

    return () => {
      cancelled = true
      cleanupFns.forEach((fn) => {
        try {
          fn()
        } catch (error) {
          console.error('[DataProvider] Cleanup error:', error)
        }
      })
    }
  }, [refreshProxy])

  const proxiesValue = useMemo(
    () => ({
      proxies: proxiesData,
      proxyProviders: proxyProviders || {},
    }),
    [proxiesData, proxyProviders],
  )

  const rulesValue = useMemo(
    () => ({
      rules: rulesData?.rules ?? [],
      ruleProviders: ruleProviders?.providers || {},
    }),
    [rulesData, ruleProviders],
  )

  const clashConfigValue = useMemo(() => ({ clashConfig }), [clashConfig])

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
      systemProxyAddress: calculateSystemProxyAddress(),
    }
  }, [sysproxy, verge, clashConfig])

  const refreshersValue = useMemo(
    () => ({
      refreshProxy,
      refreshClashConfig,
      refreshRules,
      refreshProxyProviders,
      refreshRuleProviders,
    }),
    [
      refreshProxy,
      refreshClashConfig,
      refreshRules,
      refreshProxyProviders,
      refreshRuleProviders,
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
