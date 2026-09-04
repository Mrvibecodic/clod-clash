import { useEffect, useMemo, useRef } from 'react'

import { useRuntimeConfig } from '@/hooks/use-clash'
import { useVerge } from '@/hooks/use-verge'
import { useAppRefreshers, useProxiesData } from '@/providers/app-data-context'
import delayManager from '@/services/delay'
import { debugLog } from '@/utils/debug'

import { filterSort } from './use-filter-sort'
import {
  DEFAULT_STATE,
  useHeadStateNew,
  type HeadState,
} from './use-head-state'
import { useWindowWidth } from './use-window-width'

// Определение интерфейса элемента прокси
interface IProxyItem {
  name: string
  type: string
  udp: boolean
  xudp: boolean
  tfo: boolean
  mptcp: boolean
  smux: boolean
  history: {
    time: string
    delay: number
  }[]
  provider?: string
  testUrl?: string
  [key: string]: any // Индексная сигнатура для прочих возможных свойств
}

// Тип группы прокси
type ProxyGroup = {
  name: string
  type: string
  udp: boolean
  xudp: boolean
  tfo: boolean
  mptcp: boolean
  smux: boolean
  history: {
    time: string
    delay: number
  }[]
  now: string
  all: IProxyItem[]
  hidden?: boolean
  icon?: string
  testUrl?: string
  provider?: string
}

export interface IRenderItem {
  // group | head | item | empty | item col
  type: 0 | 1 | 2 | 3 | 4
  key: string
  group: ProxyGroup
  proxy?: IProxyItem
  col?: number
  proxyCol?: IProxyItem[]
  headState?: HeadState
  // Поддержка иконки и прочих метаданных
  icon?: string
  provider?: string
  testUrl?: string
}

type GroupCache = {
  now: string
  all: IProxyItem[]
  headState: HeadState
  col: number
  latencyTimeout: number | undefined
  items: IRenderItem[]
}

// Оптимизированный расчёт раскладки колонок
const calculateColumns = (width: number, configCol: number): number => {
  if (configCol > 0 && configCol < 6) return configCol

  if (width > 1920) return 5
  if (width > 1450) return 4
  if (width > 1024) return 3
  if (width > 900) return 2
  if (width >= 600) return 2
  return 1
}

// Оптимизированная логика группировки
const groupProxies = <T = any>(list: T[], size: number): T[][] => {
  return list.reduce((acc, item) => {
    const lastGroup = acc[acc.length - 1]
    if (!lastGroup || lastGroup.length >= size) {
      acc.push([item])
    } else {
      lastGroup.push(item)
    }
    return acc
  }, [] as T[][])
}

export const useRenderList = (
  mode: string,
  isChainMode?: boolean,
  selectedGroup?: string | null,
) => {
  // Используем глобальный поставщик данных
  const { proxies: proxiesData } = useProxiesData()
  const { refreshProxy } = useAppRefreshers()
  const { verge } = useVerge()
  const { width } = useWindowWidth()
  const [headStates, setHeadState] = useHeadStateNew()
  const latencyTimeout = verge?.default_latency_timeout

  // Получаем конфиг времени выполнения для режима цепочки прокси
  const { data: runtimeConfig } = useRuntimeConfig(!!isChainMode)

  // Считаем число колонок
  const col = useMemo(
    () => calculateColumns(width, verge?.proxy_layout_column || 6),
    [width, verge?.proxy_layout_column],
  )

  // Убеждаемся, что данные прокси загружены
  useEffect(() => {
    if (!proxiesData) return
    const { groups, proxies } = proxiesData

    if (
      (mode === 'rule' && !groups.length) ||
      (mode === 'global' && proxies.length < 2)
    ) {
      // clod:Э11-05 — обновление может отклониться «ядро ещё не готово»; это
      // ожидаемо и лечится следующим тиком, необработанным отказом шуметь незачем.
      const handle = setTimeout(() => {
        refreshProxy().catch(() => {})
      }, 500)
      return () => clearTimeout(handle)
    }
  }, [proxiesData, mode, refreshProxy])

  // Автоматический расчёт задержки узлов в режиме цепочки прокси
  useEffect(() => {
    if (!isChainMode || !runtimeConfig) return

    const allProxies: IProxyItem[] = Object.values(
      (runtimeConfig as any).proxies || {},
    )
    if (allProxies.length === 0) return

    // clod:Э11-12 — слушатель срабатывает на КАЖДЫЙ завершённый замер, а замеров
    // столько же, сколько узлов: на большой подписке это десятки перевалидаций
    // интерфейса в секунду, по два запроса каждая. Замеры приходят пачками, и
    // человеку хватает одного обновления на пачку — копим их и обновляем раз в
    // полсекунды.
    let pendingRefresh: ReturnType<typeof setTimeout> | undefined
    const groupListener = () => {
      if (pendingRefresh) return
      pendingRefresh = setTimeout(() => {
        pendingRefresh = undefined
        debugLog('[ChainMode] Задержки обновлены, обновляем интерфейс')
        refreshProxy().catch(() => {})
      }, 500)
    }

    delayManager.setGroupListener('chain-mode', groupListener)

    const calculateDelays = async () => {
      try {
        const timeout = verge?.default_latency_timeout || 10000

        debugLog(
          `[ChainMode] Начало расчёта задержки для ${allProxies.length} узлов`,
        )

        // delayManager считает задержку; после расчёта каждого узла слушатель
        // автоматически обновляет интерфейс
        delayManager.checkListDelay(allProxies, 'chain-mode', timeout)
      } catch (error) {
        console.error('Failed to calculate delays for chain mode:', error)
      }
    }

    // Отложенный запуск, чтобы не блокировать
    const handle = setTimeout(calculateDelays, 100)

    return () => {
      clearTimeout(handle)
      if (pendingRefresh) clearTimeout(pendingRefresh)
      // Удаляем слушатель группы
      delayManager.removeGroupListener('chain-mode')
    }
  }, [isChainMode, runtimeConfig, verge?.default_latency_timeout, refreshProxy])

  const groupCacheRef = useRef<Map<string, GroupCache>>(new Map())
  const prevListRef = useRef<IRenderItem[]>([])

  // Формируем список для рендера
  const renderList: IRenderItem[] = useMemo(() => {
    if (!proxiesData) return []

    // В режиме цепочки прокси показываем группы прокси и их узлы
    if (isChainMode && runtimeConfig && mode === 'rule') {
      // Используем группы прокси обычного режима правил
      const allGroups = proxiesData.groups.length
        ? proxiesData.groups
        : [proxiesData.global!]

      // Если выбрана конкретная группа прокси, показываем только её узлы
      if (selectedGroup) {
        const targetGroup = allGroups.find((g: any) => g.name === selectedGroup)
        if (targetGroup) {
          const proxies = filterSort(
            targetGroup.all,
            targetGroup.name,
            '',
            0,
            latencyTimeout,
          )

          if (col > 1) {
            return groupProxies(proxies, col).map((proxyCol, colIndex) => ({
              type: 4,
              key: `chain-col-${selectedGroup}-${colIndex}`,
              group: targetGroup,
              headState: DEFAULT_STATE,
              col,
              proxyCol,
              provider: proxyCol[0]?.provider,
            }))
          } else {
            return proxies.map((proxy) => ({
              type: 2,
              key: `chain-${selectedGroup}-${proxy!.name}`,
              group: targetGroup,
              proxy,
              headState: DEFAULT_STATE,
              provider: proxy.provider,
            }))
          }
        }
        return []
      }

      // Если конкретная группа не выбрана, показываем узлы первой группы (если группы есть)
      if (allGroups.length > 0) {
        const firstGroup = allGroups[0]
        const proxies = filterSort(
          firstGroup.all,
          firstGroup.name,
          '',
          0,
          latencyTimeout,
        )

        if (col > 1) {
          return groupProxies(proxies, col).map((proxyCol, colIndex) => ({
            type: 4,
            key: `chain-col-first-${colIndex}`,
            group: firstGroup,
            headState: DEFAULT_STATE,
            col,
            proxyCol,
            provider: proxyCol[0]?.provider,
          }))
        } else {
          return proxies.map((proxy) => ({
            type: 2,
            key: `chain-first-${proxy!.name}`,
            group: firstGroup,
            proxy,
            headState: DEFAULT_STATE,
            provider: proxy.provider,
          }))
        }
      }

      // Если групп нет, показываем все узлы
      const allProxies: IProxyItem[] = allGroups.flatMap(
        (group: any) => group.all,
      )

      // Получаем данные о задержке для каждого узла
      const proxiesWithDelay = allProxies.map((proxy) => {
        const delay = delayManager.getDelay(proxy.name, 'chain-mode')
        return {
          ...proxy,
          // Если у delayManager есть данные о задержке, обновляем history
          history:
            delay >= 0
              ? [{ time: new Date().toISOString(), delay }]
              : proxy.history || [],
        }
      })

      // Создаём виртуальную группу для всех узлов
      const virtualGroup: ProxyGroup = {
        name: 'All Proxies',
        type: 'Selector',
        udp: false,
        xudp: false,
        tfo: false,
        mptcp: false,
        smux: false,
        history: [],
        now: '',
        all: proxiesWithDelay,
      }

      if (col > 1) {
        return groupProxies(proxiesWithDelay, col).map(
          (proxyCol, colIndex) => ({
            type: 4,
            key: `chain-col-all-${colIndex}`,
            group: virtualGroup,
            headState: DEFAULT_STATE,
            col,
            proxyCol,
            provider: proxyCol[0]?.provider,
          }),
        )
      } else {
        return proxiesWithDelay.map((proxy) => ({
          type: 2,
          key: `chain-all-${proxy.name}`,
          group: virtualGroup,
          proxy,
          headState: DEFAULT_STATE,
          provider: proxy.provider,
        }))
      }
    }

    // В остальных режимах (например global) в режиме цепочки прокси тоже показываем все узлы
    if (isChainMode && runtimeConfig) {
      // Получаем список proxies напрямую из конфига времени выполнения (нужно приведение типа)
      const allProxies: IProxyItem[] = Object.values(
        (runtimeConfig as any).proxies || {},
      )

      // Получаем данные о задержке для каждого узла
      const proxiesWithDelay = allProxies.map((proxy) => {
        const delay = delayManager.getDelay(proxy.name, 'chain-mode')
        return {
          ...proxy,
          // Если у delayManager есть данные о задержке, обновляем history
          history:
            delay >= 0
              ? [{ time: new Date().toISOString(), delay }]
              : proxy.history || [],
        }
      })

      // Создаём виртуальную группу для всех узлов
      const virtualGroup: ProxyGroup = {
        name: 'All Proxies',
        type: 'Selector',
        udp: false,
        xudp: false,
        tfo: false,
        mptcp: false,
        smux: false,
        history: [],
        now: '',
        all: proxiesWithDelay,
      }

      // Возвращаем список узлов (без заголовка группы)
      if (col > 1) {
        return groupProxies(proxiesWithDelay, col).map(
          (proxyCol, colIndex) => ({
            type: 4,
            key: `chain-col-${colIndex}`,
            group: virtualGroup,
            headState: DEFAULT_STATE,
            col,
            proxyCol,
            provider: proxyCol[0]?.provider,
          }),
        )
      } else {
        return proxiesWithDelay.map((proxy) => ({
          type: 2,
          key: `chain-${proxy.name}`,
          group: virtualGroup,
          proxy,
          headState: DEFAULT_STATE,
          provider: proxy.provider,
        }))
      }
    }

    // Логика рендера обычного режима
    const useRule = mode === 'rule' || mode === 'script'
    const renderGroups =
      useRule && proxiesData.groups.length
        ? proxiesData.groups
        : [proxiesData.global!]

    const cache = groupCacheRef.current
    let anyChanged = false

    const retList = renderGroups.flatMap((group: ProxyGroup) => {
      const headState = headStates[group.name] || DEFAULT_STATE
      const cached = cache.get(group.name)

      if (
        cached &&
        cached.now === group.now &&
        cached.all === group.all &&
        cached.headState === headState &&
        cached.col === col &&
        cached.latencyTimeout === latencyTimeout
      ) {
        return cached.items
      }

      anyChanged = true
      const ret: IRenderItem[] = [
        {
          type: 0,
          key: group.name,
          group,
          headState,
          icon: group.icon,
          testUrl: group.testUrl,
        },
      ]

      if (headState?.open || !useRule) {
        const proxies = filterSort(
          group.all,
          group.name,
          headState.filterText,
          headState.sortType,
          latencyTimeout,
          {
            matchCase: headState.filterMatchCase,
            matchWholeWord: headState.filterMatchWholeWord,
            useRegularExpression: headState.filterUseRegularExpression,
          },
        )

        // В глобальном режиме добавляем заголовок группы
        if (!useRule) {
          ret.push({
            type: 1,
            key: `head-${group.name}`,
            group,
            headState,
          })
        }

        if (!proxies.length) {
          ret.push({
            type: 3,
            key: `empty-${group.name}`,
            group,
            headState,
          })
        } else if (col > 1) {
          ret.push(
            ...groupProxies(proxies, col).map((proxyCol, colIndex) => ({
              type: 4 as const,
              key: `col-${group.name}-${proxyCol[0].name}-${colIndex}`,
              group,
              headState,
              col,
              proxyCol,
              provider: proxyCol[0].provider,
            })),
          )
        } else {
          ret.push(
            ...proxies.map((proxy) => ({
              type: 2 as const,
              key: `${group.name}-${proxy!.name}`,
              group,
              proxy,
              headState,
              provider: proxy.provider,
            })),
          )
        }
      }

      cache.set(group.name, {
        now: group.now,
        all: group.all,
        headState,
        col,
        latencyTimeout,
        items: ret,
      })
      return ret
    })

    const filtered = !useRule
      ? retList.slice(1)
      : retList.filter((item: IRenderItem) => !item.group.hidden)

    if (!anyChanged && prevListRef.current.length === filtered.length) {
      return prevListRef.current
    }
    prevListRef.current = filtered
    return filtered
  }, [
    headStates,
    proxiesData,
    mode,
    col,
    isChainMode,
    runtimeConfig,
    selectedGroup,
    latencyTimeout,
  ])

  return {
    renderList,
    onProxies: refreshProxy,
    onHeadState: setHeadState,
    currentColumns: col,
  }
}

// Рекомендация по оптимизации: при больших объёмах данных использовать виртуальный
// скролл (уже реализован в компоненте ProxyGroups), здесь дополнительная обработка не нужна.
