import { Box, Button, CircularProgress, Typography } from '@mui/material'
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
import { emptyState } from '@/utils/empty-state'
import { errorReason } from '@/utils/error-explanation'
import { enumText } from '@/utils/plugin-enum'
import { policyName } from '@/utils/rule-labels'

const RulesPage = () => {
  const { t } = useTranslation()
  const { rules = [], rulesLoaded, rulesError } = useRulesData()
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
      match(
        `${enumText(item.type)},${item.payload ?? ''},${policyName(t, item.proxy)}`,
      ),
    )
  }, [rules, match, t])

  const empty = emptyState({
    coreDown: isCoreDown,
    failed: Boolean(rulesError),
    loading: !rulesLoaded,
    searching,
    hasData: rules.length > 0,
  })

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
      ) : empty === 'coreDown' ? (
        <Box sx={{ m: 'auto', px: 2 }}>
          <CoreStatus />
        </Box>
      ) : empty === 'failed' ? (
        <BaseEmpty
          textKey="rules.page.loadFailed"
          extra={
            <>
              <Typography
                variant="caption"
                sx={{ mt: 0.5, mb: 1, textAlign: 'center' }}
              >
                {errorReason(rulesError, t)}
              </Typography>
              <Button
                size="small"
                onClick={() => void refreshRules().catch(() => {})}
              >
                {t('shared.actions.retry')}
              </Button>
            </>
          }
        />
      ) : empty === 'loading' ? (
        <Box sx={{ m: 'auto' }}>
          <CircularProgress size={24} />
        </Box>
      ) : (
        <BaseEmpty
          textKey={
            empty === 'notFound'
              ? 'rules.modals.editor.list.notFound'
              : undefined
          }
        />
      )}
    </BasePage>
  )
}

export default RulesPage
