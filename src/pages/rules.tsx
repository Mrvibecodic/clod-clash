import { Box, CircularProgress } from '@mui/material'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  BaseEmpty,
  BasePage,
  BaseSearchBox,
  VirtualList,
  type VirtualListHandle,
} from '@/components/base'
import { CoreStatus } from '@/components/home/core-status'
import { ScrollTopButton } from '@/components/layout/scroll-top-button'
import { ProviderButton } from '@/components/rule/provider-button'
import RuleItem from '@/components/rule/rule-item'
import { useSystemState } from '@/hooks/use-system-state'
import { useVisibility } from '@/hooks/use-visibility'
import { useAppRefreshers, useRulesData } from '@/providers/app-data-context'
import { enumText } from '@/utils/plugin-enum'

const RulesPage = () => {
  const { t } = useTranslation()
  const { rules = [], rulesLoaded } = useRulesData()
  const { isCoreDown } = useSystemState()
  const [searching, setSearching] = useState(false)
  const { refreshRules, refreshRuleProviders } = useAppRefreshers()
  const [match, setMatch] = useState(() => (_: string) => true)
  const virtuosoRef = useRef<VirtualListHandle>(null)
  const [showScrollTop, setShowScrollTop] = useState(false)
  const pageVisible = useVisibility()

  // Правила перечитываются, когда окно показано: в трее список никому не нужен
  useEffect(() => {
    if (!pageVisible) return

    refreshRules()
    refreshRuleProviders()
  }, [refreshRules, refreshRuleProviders, pageVisible])

  const filteredRules = useMemo(() => {
    const rulesWithLineNo = rules.map((item, index) => ({
      ...item,
      // UI-only derived data; keep app context/SWR data immutable
      lineNo: index + 1,
    }))

    return rulesWithLineNo.filter((item) =>
      match(`${enumText(item.type)},${item.payload ?? ''},${item.proxy}`),
    )
  }, [rules, match])

  const handleScroll = useCallback((e: Event) => {
    setShowScrollTop((e.target as HTMLElement).scrollTop > 100)
  }, [])

  const scrollToTop = () => {
    virtuosoRef.current?.scrollTo({ top: 0, behavior: 'smooth' })
  }

  return (
    <BasePage
      full
      title={t('rules.page.title')}
      contentStyle={{
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        overflow: 'auto',
      }}
      header={
        <Box sx={{ display: 'flex', alignItems: 'center', gap: 1 }}>
          <ProviderButton />
        </Box>
      }
    >
      <Box
        sx={{
          pt: 1,
          mb: 0.5,
          mx: '10px',
          height: '36px',
          display: 'flex',
          alignItems: 'center',
        }}
      >
        <BaseSearchBox
          onSearch={(match, state) => {
            setMatch(() => match)
            setSearching(state.text !== '')
          }}
        />
      </Box>

      {filteredRules && filteredRules.length > 0 ? (
        <>
          <VirtualList
            ref={virtuosoRef}
            count={filteredRules.length}
            estimateSize={40}
            renderItem={(i) => <RuleItem value={filteredRules[i]} />}
            style={{ flex: 1 }}
            onScroll={handleScroll}
          />
          <ScrollTopButton onClick={scrollToTop} show={showScrollTop} />
        </>
      ) : isCoreDown ? (
        <Box sx={{ m: 'auto', px: 2 }}>
          <CoreStatus />
        </Box>
      ) : !rulesLoaded ? (
        <Box sx={{ m: 'auto' }}>
          <CircularProgress size={24} />
        </Box>
      ) : (
        <BaseEmpty
          textKey={
            searching && rules.length > 0
              ? 'rules.modals.editor.list.notFound'
              : undefined
          }
        />
      )}
    </BasePage>
  )
}

export default RulesPage
