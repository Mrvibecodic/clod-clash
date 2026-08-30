import { Box } from '@mui/material'
import { lazy, Suspense, type ComponentType } from 'react'

import { BaseLoading } from '@/components/base'
import { ensureLanguageSections } from '@/services/i18n'

import { navigationItems } from './_navigation-meta'
import HomePage from './home'

type NavigationItem = {
  label: (typeof navigationItems)[keyof typeof navigationItems]['label']
  path: string
  Component: ComponentType
  preload?: () => Promise<{ default: ComponentType }>
}

const waitForWarmupIdle = (signal: AbortSignal) =>
  new Promise<void>((resolve) => {
    let idleId: number | undefined
    let timeoutId: number | undefined

    const cleanup = () => {
      signal.removeEventListener('abort', finish)
      if (idleId !== undefined) {
        window.cancelIdleCallback(idleId)
      }
      if (timeoutId !== undefined) {
        window.clearTimeout(timeoutId)
      }
    }

    const finish = () => {
      cleanup()
      resolve()
    }

    if (signal.aborted) {
      resolve()
      return
    }

    signal.addEventListener('abort', finish, { once: true })

    if (window.requestIdleCallback) {
      idleId = window.requestIdleCallback(finish, { timeout: 500 })
    } else {
      timeoutId = window.setTimeout(finish, 120)
    }
  })

const createRoutePreload = (
  load: () => Promise<{ default: ComponentType }>,
  sections?: string | readonly string[],
) => {
  let componentPromise: Promise<{ default: ComponentType }> | undefined

  const loadComponent = () => {
    componentPromise ??= load().catch((error) => {
      componentPromise = undefined
      throw error
    })

    return componentPromise
  }

  if (!sections) {
    return loadComponent
  }

  return async () => {
    const [component] = await Promise.all([
      loadComponent(),
      ensureLanguageSections(sections),
    ])
    return component
  }
}

const createLazyRoute = (
  load: () => Promise<{ default: ComponentType }>,
  sections?: string | readonly string[],
) => {
  const preload = createRoutePreload(load, sections)
  const Component = lazy(preload)
  const LazyRoute = () => (
    <Suspense
      fallback={
        <Box
          sx={{
            display: 'flex',
            height: '100%',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <BaseLoading />
        </Box>
      }
    >
      <Component />
    </Suspense>
  )

  return { Component: LazyRoute, preload }
}

export const preloadLogsPage = createRoutePreload(
  () => import('./logs'),
  'logs',
)

export const navItems: NavigationItem[] = [
  {
    ...navigationItems.home,
    Component: HomePage,
  },
  {
    ...navigationItems.proxies,
    ...createLazyRoute(() => import('./proxies')),
  },
  {
    ...navigationItems.profiles,
    ...createLazyRoute(() => import('./profiles'), 'rules'),
  },
  {
    ...navigationItems.connections,
    ...createLazyRoute(() => import('./connections'), 'connections'),
  },
  {
    ...navigationItems.rules,
    ...createLazyRoute(() => import('./rules'), 'rules'),
  },
  {
    ...navigationItems.logs,
    Component: () => null /* LogsPage rendered in Layout only on /logs route */,
    preload: preloadLogsPage,
  },
  {
    ...navigationItems.unlock,
    ...createLazyRoute(() => import('./unlock')),
  },
  {
    ...navigationItems.settings,
    ...createLazyRoute(() => import('./settings')),
  },
]

export const preloadNavigationRoutes = async (signal: AbortSignal) => {
  await waitForWarmupIdle(signal)
  if (signal.aborted) {
    return
  }

  await Promise.all(
    navItems.map((item) => {
      const preload = 'preload' in item ? item.preload : undefined
      return preload?.().catch(() => {})
    }),
  )
}
