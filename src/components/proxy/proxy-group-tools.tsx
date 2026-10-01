import NetworkCheckRounded from '@mui/icons-material/NetworkCheckRounded'
import { Box, IconButton, TextField } from '@mui/material'
import { useDebounceFn } from 'ahooks'
import { memo, useEffect, useRef } from 'react'
import { flushSync } from 'react-dom'
import { useTranslation } from 'react-i18next'

import delayManager from '@/services/delay'
import { showNotice } from '@/services/notice-service'
import { isValidUrl } from '@/utils/network'

import { BaseSearchBox, type SearchState } from '../base'

import { ProxyToolsMenu } from './proxy-tools-menu'
import type { HeadState } from './use-head-state'

interface Props {
  groupName: string
  headState: HeadState
  onLocation: () => void
  onCheckDelay: () => void
  onHeadState: (val: Partial<HeadState>) => void
}

export const ProxyGroupTools = memo(function ProxyGroupTools(props: Props) {
  const { groupName, headState, onCheckDelay, onHeadState, onLocation } = props

  const {
    filterText,
    textState,
    testUrl,
    filterMatchCase,
    filterMatchWholeWord,
    filterUseRegularExpression,
  } = headState

  const { t } = useTranslation()
  const inputRef = useRef<HTMLInputElement>(null)
  // clod:УП-33 — негодный адрес подсвечивается в самом поле, а не только по кнопке проверки.
  const badTestUrl =
    testUrl !== undefined && testUrl.trim() !== '' && !isValidUrl(testUrl)

  useEffect(() => {
    const custom = testUrl?.trim()
    if (custom) {
      delayManager.setUrl(groupName, custom)
    } else {
      delayManager.clearUrl(groupName)
    }
  }, [groupName, testUrl])

  const { run: applyFilter, flush: flushFilter } = useDebounceFn(
    (state: SearchState) => {
      onHeadState({
        filterText: state.text,
        filterMatchCase: state.matchCase,
        filterMatchWholeWord: state.matchWholeWord,
        filterUseRegularExpression: state.useRegularExpression,
      })
    },
    { wait: 600 },
  )

  useEffect(() => {
    if (textState !== 'filter') flushFilter()
  }, [textState, flushFilter])
  useEffect(() => () => flushFilter(), [flushFilter])

  return (
    <Box
      sx={{
        display: 'flex',
        justifyContent: 'end',
        alignItems: 'center',
        gap: 0.5,
        height: 36,
        flex: 1,
        ml: 2,
      }}
    >
      {textState === 'filter' && (
        <Box sx={{ flex: '1 1 auto' }}>
          <BaseSearchBox
            inputRef={inputRef}
            defaultValue={filterText}
            matchCase={filterMatchCase}
            matchWholeWord={filterMatchWholeWord}
            useRegularExpression={filterUseRegularExpression}
            onClick={(e) => {
              e.preventDefault()
              e.stopPropagation()
            }}
            onSearch={(_, state) => applyFilter(state)}
          />
        </Box>
      )}

      {textState === 'url' && (
        <TextField
          autoComplete="new-password"
          hiddenLabel
          autoSave="off"
          inputRef={inputRef}
          value={testUrl}
          size="small"
          variant="outlined"
          error={badTestUrl}
          title={badTestUrl ? t('proxies.page.messages.badTestUrl') : undefined}
          placeholder={t('proxies.page.placeholders.delayCheckUrl')}
          onClick={(e) => {
            e.preventDefault()
            e.stopPropagation()
          }}
          onChange={(e) => onHeadState({ testUrl: e.target.value })}
          sx={{ flex: '1 1 auto', input: { py: 0.65, px: 1 } }}
        />
      )}
      <IconButton
        size="small"
        color="inherit"
        title={t('proxies.page.tooltips.delayCheck')}
        onClick={(e) => {
          e.preventDefault()
          e.stopPropagation()
          // eslint-disable-next-line @eslint-react/dom-no-flush-sync
          if (!headState.open) flushSync(() => onHeadState({ open: true }))

          if (testUrl?.trim() && textState !== 'filter') {
            onHeadState({ textState: 'url' })
          }
          if (testUrl?.trim() && !isValidUrl(testUrl)) {
            showNotice.warning('proxies.page.messages.invalidTestUrl')
          }
          onCheckDelay()
        }}
      >
        <NetworkCheckRounded fontSize="inherit" />
      </IconButton>

      <ProxyToolsMenu
        headState={headState}
        onHeadState={onHeadState}
        onLocation={onLocation}
        ensureOpen={() => {
          // eslint-disable-next-line @eslint-react/dom-no-flush-sync
          if (!headState.open) flushSync(() => onHeadState({ open: true }))
        }}
        onFieldShown={() => setTimeout(() => inputRef.current?.focus())}
        iconSize="inherit"
      />
    </Box>
  )
})
