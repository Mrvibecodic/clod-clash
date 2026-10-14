'use client'
import SearchDialog from '@/components/search'
import { RootProvider, type RootProviderProps } from 'fumadocs-ui/provider/next'
import type { ReactNode } from 'react'

export function Provider({
  children,
  i18n,
}: {
  children: ReactNode
  i18n?: RootProviderProps['i18n']
}) {
  return (
    <RootProvider search={{ SearchDialog }} i18n={i18n}>
      {children}
    </RootProvider>
  )
}
