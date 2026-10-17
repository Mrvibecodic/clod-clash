import { createBrowserRouter, RouteObject } from 'react-router'

import { RouteCrash } from '@/components/base/base-error-boundary'

import Layout from './_layout'
import { navItems } from './_navigation'

export const router = createBrowserRouter([
  {
    path: '/',
    Component: Layout,
    ErrorBoundary: RouteCrash,
    children: navItems.map(
      (item) =>
        ({
          path: item.path,
          Component: item.Component,
        }) as RouteObject,
    ),
  },
])
