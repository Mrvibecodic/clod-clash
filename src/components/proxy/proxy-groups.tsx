import { defaultRangeExtractor, useVirtualizer } from '@tanstack/react-virtual'
import { useLockFn } from 'ahooks'
import { throttle } from 'lodash-es'
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react'

import {
  BaseEmpty,
  BaseLoading,
  StickyVirtualList,
  type StickyVirtualListHandle,
} from '@/components/base'
import { useProxySelection } from '@/hooks/use-proxy-selection'
import { useVerge } from '@/hooks/use-verge'
import { useVisibility } from '@/hooks/use-visibility'
import { useProxiesData } from '@/providers/app-data-context'
import { calcuProxies } from '@/services/cmds'
import delayManager from '@/services/delay'
import { useQuery } from '@/services/query-client'
import { debugLog } from '@/utils/debug'

import {
  DEFAULT_HOVER_DELAY,
  ProxyGroupNavigator,
} from './proxy-group-navigator'
import { ProxyRender } from './proxy-render'
import { type IRenderItem, useRenderList } from './use-render-list'

const ProxyGroupsChain = lazy(() =>
  import('./proxy-groups-chain').then((m) => ({
    default: m.ProxyGroupsChain,
  })),
)

function useStableCallback<T extends (...args: any[]) => any>(fn: T): T {
  const ref = useRef(fn)
  ref.current = fn
  return useCallback((...args: Parameters<T>) => ref.current(...args), []) as T
}

interface Props {
  mode: string
  isChainMode?: boolean
  chainConfigData?: string | null
}

function useProxyRenderState(
  mode: string,
  isChainMode: boolean,
  activeSelectedGroup: string | null,
) {
  const { verge } = useVerge()
  const { renderList, onProxies, onHeadState } = useRenderList(
    mode,
    isChainMode,
    activeSelectedGroup,
  )
  const scrollPositionKey = useMemo(
    () =>
      isChainMode
        ? `${mode}:chain:${activeSelectedGroup ?? 'all'}`
        : `${mode}:normal`,
    [activeSelectedGroup, isChainMode, mode],
  )

  const getGroupHeadState = useCallback(
    (groupName: string) => {
      const headItem = renderList.find(
        (item) => item.type === 1 && item.group?.name === groupName,
      )
      return headItem?.headState
    },
    [renderList],
  )

  const timeout = verge?.default_latency_timeout || 10000

  const handleCheckAll = useStableCallback(
    useLockFn(async (groupName: string) => {
      debugLog(
        `[ProxyGroups] Начало тестирования всех задержек, группа: ${groupName}`,
      )

      const proxies = renderList
        .filter(
          (e) => e.group?.name === groupName && (e.type === 2 || e.type === 4),
        )
        .flatMap((e) => e.proxyCol || e.proxy!)
        .filter(Boolean)

      debugLog(`[ProxyGroups] Найдено прокси: ${proxies.length}`)

      debugLog(
        `[ProxyGroups] URL теста: ${delayManager.getUrl(groupName)}, тайм-аут: ${timeout}ms`,
      )

      try {
        await delayManager.checkListDelay(proxies, groupName, timeout)
        debugLog(
          `[ProxyGroups] Тестирование задержки завершено, группа: ${groupName}`,
        )
      } catch (error) {
        console.error(
          `[ProxyGroups] Ошибка тестирования задержки, группа: ${groupName}`,
          error,
        )
      } finally {
        const headState = getGroupHeadState(groupName)
        if (headState?.sortType === 1) {
          onHeadState(groupName, { sortType: headState.sortType })
        }
        onProxies()
      }
    }),
  )

  const saveScrollPosition = useCallback(
    (scrollTop: number) => {
      try {
        const saved = localStorage.getItem('proxy-scroll-positions')
        const scrollPositions = saved ? JSON.parse(saved) : {}
        scrollPositions[scrollPositionKey] = scrollTop
        localStorage.setItem(
          'proxy-scroll-positions',
          JSON.stringify(scrollPositions),
        )
      } catch (e) {
        console.error('Error saving scroll position:', e)
      }
    },
    [scrollPositionKey],
  )

  const getScrollPosition = useCallback(() => {
    try {
      const savedPositions = localStorage.getItem('proxy-scroll-positions')
      if (savedPositions) {
        const positions = JSON.parse(savedPositions)
        const savedPosition = positions[scrollPositionKey]
        return savedPosition ?? 0
      }
    } catch (e) {
      console.error('Error restoring scroll position:', e)
    }
  }, [scrollPositionKey])

  return {
    verge,
    renderList,
    onProxies,
    onHeadState,
    handleCheckAll,
    saveScrollPosition,
    getScrollPosition,
  }
}

function ChainProxyGroups(props: {
  mode: string
  chainConfigData?: string | null
}) {
  const { mode, chainConfigData } = props
  const { proxies: proxiesData } = useProxiesData()
  const [selectedGroup, setSelectedGroup] = useState<string | null>(null)

  const availableGroups = useMemo(() => {
    const groups = proxiesData?.groups
    if (!groups) return []
    return groups.filter(
      (group: any) => group.type === 'Selector' || group.type === 'URLTest',
    )
  }, [proxiesData?.groups])

  const defaultRuleGroup = useMemo(() => {
    if (mode === 'rule' && availableGroups.length > 0) {
      return availableGroups[0].name
    }
    return null
  }, [availableGroups, mode])

  const activeSelectedGroup = selectedGroup ?? defaultRuleGroup
  const {
    renderList,
    onHeadState,
    handleCheckAll,
    getScrollPosition,
    saveScrollPosition,
  } = useProxyRenderState(mode, true, activeSelectedGroup)

  const parentRef = useRef<HTMLDivElement>(null)
  const scrollTopRef = useRef(0)
  const showScrollTopRef = useRef(false)
  const activeStickyIndexRef = useRef<number | null>(null)
  const [showScrollTop, setShowScrollTop] = useState(false)
  const stickyGroupIndexes = useMemo(
    () =>
      renderList.flatMap((item, index) =>
        item.type === 0 && !item.group.hidden ? [index] : [],
      ),
    [renderList],
  )

  const rangeExtractor = useCallback(
    (range: Parameters<typeof defaultRangeExtractor>[0]) => {
      const activeStickyIndex = [...stickyGroupIndexes]
        .reverse()
        .find((index) => index <= range.startIndex)
      activeStickyIndexRef.current = activeStickyIndex ?? null

      const indexes = defaultRangeExtractor(range)
      return activeStickyIndex == null || indexes.includes(activeStickyIndex)
        ? indexes
        : [activeStickyIndex, ...indexes]
    },
    [stickyGroupIndexes],
  )

  const virtualizer = useVirtualizer({
    count: renderList.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 56,
    overscan: 15,
    getItemKey: (index) => renderList[index]?.key ?? index,
    rangeExtractor,
  })
  const virtualItems = virtualizer.getVirtualItems()
  const activeStickyIndex = activeStickyIndexRef.current

  useLayoutEffect(() => {
    if (renderList.length === 0) return
    const node = parentRef.current
    if (!node) return

    const savedPosition = getScrollPosition()
    if (savedPosition !== undefined) {
      node.scrollTop = savedPosition
      scrollTopRef.current = savedPosition
      const nextShowScrollTop = savedPosition > 100
      showScrollTopRef.current = nextShowScrollTop
      queueMicrotask(() => setShowScrollTop(nextShowScrollTop))
    }
  }, [renderList.length, getScrollPosition])

  const saveScrollPositionThrottled = useMemo(
    () => throttle(saveScrollPosition, 500),
    [saveScrollPosition],
  )

  const handleScroll = useCallback(
    (event: Event) => {
      const target = event.target as HTMLElement | null
      const nextScrollTop = target?.scrollTop ?? 0
      const nextShowScrollTop = nextScrollTop > 100
      scrollTopRef.current = nextScrollTop

      if (showScrollTopRef.current !== nextShowScrollTop) {
        showScrollTopRef.current = nextShowScrollTop
        setShowScrollTop(nextShowScrollTop)
      }

      saveScrollPositionThrottled(nextScrollTop)
    },
    [saveScrollPositionThrottled],
  )

  useEffect(() => {
    const node = parentRef.current
    if (!node) return

    const listener = handleScroll as EventListener
    const options: AddEventListenerOptions = { passive: true }

    node.addEventListener('scroll', listener, options)

    return () => {
      saveScrollPosition(scrollTopRef.current)
      node.removeEventListener('scroll', listener, options)
    }
  }, [handleScroll, saveScrollPosition])

  const scrollToTop = useCallback(() => {
    parentRef.current?.scrollTo?.({
      top: 0,
      behavior: 'smooth',
    })
    scrollTopRef.current = 0
  }, [])

  const handleLocation = useStableCallback((group: IProxyGroupItem) => {
    if (!group) return
    const { name, now } = group

    const index = renderList.findIndex(
      (item) =>
        item.group?.name === name &&
        ((item.type === 2 && item.proxy?.name === now) ||
          (item.type === 4 &&
            item.proxyCol?.some((proxy) => proxy.name === now))),
    )

    if (index >= 0) {
      virtualizer.scrollToIndex(index, {
        align: 'center',
        behavior: 'smooth',
      })
    }
  })

  return (
    <Suspense fallback={<BaseLoading />}>
      <ProxyGroupsChain
        mode={mode}
        chainConfigData={chainConfigData}
        availableGroups={availableGroups}
        activeSelectedGroup={activeSelectedGroup}
        showScrollTop={showScrollTop}
        parentRef={parentRef}
        totalSize={virtualizer.getTotalSize()}
        virtualItems={virtualItems}
        renderList={renderList}
        activeStickyIndex={activeStickyIndex}
        measureElement={virtualizer.measureElement}
        onCheckAll={handleCheckAll}
        onHeadState={onHeadState}
        onLocation={handleLocation}
        onGroupSelect={setSelectedGroup}
        onScrollToTop={scrollToTop}
      />
    </Suspense>
  )
}

function NormalProxyGroups(props: { mode: string }) {
  const { mode } = props
  const stickyListRef = useRef<StickyVirtualListHandle>(null)
  const {
    verge,
    renderList,
    onProxies,
    onHeadState,
    handleCheckAll,
    getScrollPosition,
    saveScrollPosition,
  } = useProxyRenderState(mode, false, null)
  const renderFirstRef = useRef(true)
  const isRestoringRef = useRef(false)

  useLayoutEffect(() => {
    if (renderList.length === 0) return
    if (!renderFirstRef.current) return
    const node = stickyListRef.current?.getScrollElement()
    if (!node) return

    const savedPosition = getScrollPosition()
    if (!savedPosition) {
      renderFirstRef.current = false
      return
    }

    isRestoringRef.current = true
    let rafId = 0
    let attempts = 0
    const maxAttempts = 30

    const step = () => {
      const el = stickyListRef.current?.getScrollElement()
      if (!el) {
        isRestoringRef.current = false
        return
      }

      el.scrollTop = savedPosition
      attempts += 1

      const reached = Math.abs(el.scrollTop - savedPosition) <= 1
      if (reached || attempts >= maxAttempts) {
        renderFirstRef.current = false
        isRestoringRef.current = false
        return
      }

      rafId = requestAnimationFrame(step)
    }

    rafId = requestAnimationFrame(step)
    return () => {
      cancelAnimationFrame(rafId)
      isRestoringRef.current = false
    }
  }, [renderList.length, getScrollPosition])

  const saveScrollPositionThrottled = useMemo(
    () => throttle(saveScrollPosition, 500),
    [saveScrollPosition],
  )

  const handleScroll = useCallback(
    (event: Event) => {
      if (isRestoringRef.current) return
      const target = event.target as HTMLElement | null
      const nextScrollTop = target?.scrollTop ?? 0

      saveScrollPositionThrottled(nextScrollTop)
    },
    [saveScrollPositionThrottled],
  )

  useEffect(() => {
    const node = stickyListRef.current?.getScrollElement()
    if (!node) return

    const listener = handleScroll as EventListener
    const options: AddEventListenerOptions = { passive: true }

    node.addEventListener('scroll', listener, options)

    return () => {
      node.removeEventListener('scroll', listener, options)
    }
  }, [handleScroll])

  const { handleProxyGroupChange } = useProxySelection({
    onSuccess: () => {
      onProxies()
    },
    onError: (error) => {
      console.error('Ошибка переключения прокси', error)
      onProxies()
    },
  })

  const handleChangeProxy = useCallback(
    (group: IProxyGroupItem, proxy: IProxyItem) => {
      if (!['Selector', 'URLTest', 'Fallback'].includes(group.type)) return

      handleProxyGroupChange(group, proxy)
    },
    [handleProxyGroupChange],
  )

  const handleLocation = useStableCallback((group: IProxyGroupItem) => {
    if (!group) return
    const { name, now } = group

    const index = renderList.findIndex(
      (e) =>
        e.group?.name === name &&
        ((e.type === 2 && e.proxy?.name === now) ||
          (e.type === 4 && e.proxyCol?.some((p) => p.name === now))),
    )

    if (index >= 0) {
      stickyListRef.current?.scrollToIndex(index, {
        align: 'center',
        behavior: 'smooth',
      })
    }
  })

  const handleGroupLocationByName = useCallback(
    (groupName: string) => {
      const index = renderList.findIndex(
        (item) => item.type === 0 && item.group?.name === groupName,
      )

      if (index >= 0) {
        stickyListRef.current?.scrollToIndex(index, {
          align: 'start',
          behavior: 'smooth',
        })
      }
    },
    [renderList],
  )

  const proxyGroupNames = useMemo(() => {
    const names = renderList
      .filter((item) => item.type === 0 && item.group?.name)
      .map((item) => item.group!.name)
    return Array.from(new Set(names))
  }, [renderList])

  const handleGroupToggle = useCallback(
    async (group: IProxyGroupItem) => {
      const index = renderList.findIndex(
        (item) => item.type === 0 && item.group.name === group.name,
      )
      if (index < 0) return

      if (!stickyListRef.current?.isItemScrolledPastStart(index, 1)) return

      stickyListRef.current.scrollToIndex(index, {
        align: 'start',
        behavior: 'auto',
      })

      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => resolve())
      })
    },
    [renderList],
  )

  const renderGroupItem = useCallback(
    (item: IRenderItem, _index: number, stickyed: boolean) => (
      <ProxyRender
        item={item}
        stickyed={stickyed}
        onLocation={handleLocation}
        onCheckAll={handleCheckAll}
        onHeadState={async (groupName, patch) => {
          if (stickyed && patch.filterText !== undefined) {
            handleGroupLocationByName(groupName)
            await stickyListRef.current?.waitForScrollEnd()
          }
          onHeadState(groupName, patch)
        }}
        onChangeProxy={handleChangeProxy}
        onGroupToggle={handleGroupToggle}
      />
    ),
    [
      handleChangeProxy,
      handleCheckAll,
      onHeadState,
      handleLocation,
      handleGroupToggle,
      handleGroupLocationByName,
    ],
  )

  const renderProxyItem = useCallback(
    (item: IRenderItem) => (
      <ProxyRender
        key={item.key}
        item={item}
        onLocation={handleLocation}
        onCheckAll={handleCheckAll}
        onHeadState={onHeadState}
        onChangeProxy={handleChangeProxy}
      />
    ),
    [handleChangeProxy, handleCheckAll, onHeadState, handleLocation],
  )

  return (
    <div style={{ position: 'relative', height: '100%' }}>
      <StickyVirtualList
        ref={stickyListRef}
        items={renderList}
        isGroupItem={(item) => item.type === 0}
        getItemKey={(item) => item.key}
        estimateGroupItemHeight={76}
        estimateItemHeight={64}
        renderGroupItem={renderGroupItem}
        renderItem={renderProxyItem}
      />

      {mode === 'rule' && (
        <ProxyGroupNavigator
          proxyGroupNames={proxyGroupNames}
          onGroupLocation={handleGroupLocationByName}
          enableHoverJump={verge?.enable_hover_jump_navigator ?? true}
          hoverDelay={verge?.hover_jump_navigator_delay ?? DEFAULT_HOVER_DELAY}
        />
      )}
    </div>
  )
}

export const ProxyGroups = (props: Props) => {
  const { mode, isChainMode = false, chainConfigData } = props

  const pageVisible = useVisibility()
  useQuery({
    queryKey: ['getProxies'],
    queryFn: calcuProxies,
    refetchInterval: pageVisible ? 3000 : false,
    refetchIntervalInBackground: false,
    staleTime: 1500,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })

  if (mode === 'direct') {
    return <BaseEmpty textKey="proxies.page.messages.directMode" />
  }

  if (isChainMode) {
    return <ChainProxyGroups mode={mode} chainConfigData={chainConfigData} />
  }

  return <NormalProxyGroups mode={mode} />
}
