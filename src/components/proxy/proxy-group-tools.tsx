import NetworkCheckRounded from '@mui/icons-material/NetworkCheckRounded'
import {
  Box,
  IconButton,
  TextField,
  type SxProps,
  type Theme,
} from '@mui/material'
import { useDebounceFn } from 'ahooks'
import { memo, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import { useTranslation } from 'react-i18next'

import { useProfiles } from '@/hooks/use-profiles'
import delayManager from '@/services/delay'
import { showNotice } from '@/services/notice-service'
import { debugLog } from '@/utils/debug'
import { isValidUrl } from '@/utils/network'
import { toSxArray } from '@/utils/sx'

import { BaseSearchBox, type SearchState } from '../base'

import { ProxyToolsMenu } from './proxy-tools-menu'
import type { HeadState } from './use-head-state'

interface Props {
  groupName: string
  headState: HeadState
  /** В шапке группы («Правила»): поле перед кнопками, клик не сворачивает группу. */
  inHeader?: boolean
  sx?: SxProps<Theme>
  onLocation: () => void
  onCheckDelay: () => void
  onHeadState: (val: Partial<HeadState>) => void
}

const filterKey = (
  state: Pick<
    HeadState,
    | 'filterText'
    | 'filterMatchCase'
    | 'filterMatchWholeWord'
    | 'filterUseRegularExpression'
  >,
) =>
  JSON.stringify([
    state.filterText,
    !!state.filterMatchCase,
    !!state.filterMatchWholeWord,
    !!state.filterUseRegularExpression,
  ])

const stop = (e: {
  preventDefault: () => void
  stopPropagation: () => void
}) => {
  e.preventDefault()
  e.stopPropagation()
}

export const ProxyGroupTools = memo(function ProxyGroupTools(props: Props) {
  const {
    groupName,
    headState,
    inHeader = false,
    sx,
    onCheckDelay,
    onHeadState,
    onLocation,
  } = props

  const {
    filterText,
    textState,
    testUrl,
    filterMatchCase,
    filterMatchWholeWord,
    filterUseRegularExpression,
  } = headState

  const { t } = useTranslation()
  const { profiles } = useProfiles()
  const profileUid = profiles?.current
  const inputRef = useRef<HTMLInputElement>(null)
  // clod:УП-33 — негодный адрес подсвечивается в самом поле, а не только по кнопке проверки.
  const badTestUrl =
    testUrl !== undefined && testUrl.trim() !== '' && !isValidUrl(testUrl)

  // clod: в менеджер уходит только ручной ввод. `url:` группы и общий адрес из
  // настроек менеджер знает сам (configUrlMap/defaultUrl из useGroupTestUrls);
  // писать их сюда значило бы застолбить за группой нынешний `url:` — после
  // обновления подписки запись затенила бы новый.
  useEffect(() => {
    const custom = testUrl?.trim()
    if (custom) {
      delayManager.setUrl(groupName, custom)
    } else {
      delayManager.clearUrl(groupName)
    }
  }, [groupName, testUrl])

  // Поле фильтра своё, пока человек печатает. Фильтр группы сменился не им
  // (другая подписка) — поле пересоздаётся с новым текстом.
  const shownFilter = filterKey(headState)
  const ownFilterRef = useRef(shownFilter)
  const [seenFilter, setSeenFilter] = useState(shownFilter)
  const [fieldKey, setFieldKey] = useState(0)
  if (seenFilter !== shownFilter) {
    setSeenFilter(shownFilter)
    if (shownFilter !== ownFilterRef.current) setFieldKey((key) => key + 1)
  }

  const {
    run: applyFilter,
    flush: flushFilter,
    cancel: cancelFilter,
  } = useDebounceFn(
    (state: SearchState) => {
      ownFilterRef.current = filterKey({
        filterText: state.text,
        filterMatchCase: state.matchCase,
        filterMatchWholeWord: state.matchWholeWord,
        filterUseRegularExpression: state.useRegularExpression,
      })
      onHeadState({
        filterText: state.text,
        filterMatchCase: state.matchCase,
        filterMatchWholeWord: state.matchWholeWord,
        filterUseRegularExpression: state.useRegularExpression,
      })
    },
    { wait: 600 },
  )

  // Набранное в прежней подписке ей и остаётся: в новую не пишем.
  useEffect(() => cancelFilter(), [profileUid, cancelFilter])
  useEffect(() => {
    if (textState !== 'filter') flushFilter()
  }, [textState, flushFilter])
  // Набранное не теряется, когда строка уходит из списка. Очистка эффекта
  // макета идёт раньше, чем useDebounceFn сам отменяет вызов при уходе.
  useLayoutEffect(() => () => flushFilter(), [flushFilter])

  const ensureOpen = inHeader
    ? () => {
        // eslint-disable-next-line @eslint-react/dom-no-flush-sync
        if (!headState.open) flushSync(() => onHeadState({ open: true }))
      }
    : undefined

  const field =
    textState === 'filter' ? (
      <Box sx={{ flex: '1 1 auto', ml: inHeader ? 0 : 0.5 }}>
        <BaseSearchBox
          key={fieldKey}
          inputRef={inputRef}
          defaultValue={filterText}
          matchCase={filterMatchCase}
          matchWholeWord={filterMatchWholeWord}
          useRegularExpression={filterUseRegularExpression}
          onClick={stop}
          onSearch={(_, state) => applyFilter(state)}
        />
      </Box>
    ) : textState === 'url' ? (
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
        onClick={stop}
        onChange={(e) => onHeadState({ testUrl: e.target.value })}
        sx={{
          ml: inHeader ? 0 : 0.5,
          flex: '1 1 auto',
          input: { py: 0.65, px: 1 },
        }}
      />
    ) : null

  return (
    <Box
      sx={[
        { display: 'flex', alignItems: 'center', gap: 0.5 },
        inHeader && { justifyContent: 'end', height: 36, flex: 1, ml: 2 },
        ...toSxArray(sx),
      ]}
    >
      {inHeader && field}
      <IconButton
        size="small"
        color="inherit"
        title={t('proxies.page.tooltips.delayCheck')}
        onClick={(e) => {
          stop(e)
          ensureOpen?.()
          debugLog(
            `[ProxyHead] Нажата кнопка проверки задержки, группа: ${groupName}`,
          )
          // Напомнить, что проверка идёт по своему адресу
          if (testUrl?.trim() && textState !== 'filter') {
            debugLog(
              `[ProxyHead] Используется пользовательский URL для теста: ${testUrl}`,
            )
            onHeadState({ textState: 'url' })
          }
          if (testUrl?.trim() && !isValidUrl(testUrl)) {
            showNotice.warning('proxies.page.messages.invalidTestUrl')
          }
          onCheckDelay()
        }}
      >
        <NetworkCheckRounded fontSize={inHeader ? 'inherit' : undefined} />
      </IconButton>

      <ProxyToolsMenu
        headState={headState}
        onHeadState={onHeadState}
        onLocation={onLocation}
        ensureOpen={ensureOpen}
        onFieldShown={() => setTimeout(() => inputRef.current?.focus())}
        iconSize={inHeader ? 'inherit' : undefined}
      />
      {!inHeader && field}
    </Box>
  )
})
