import { ClearRounded } from '@mui/icons-material'
import { Box, SvgIcon, TextField, styled, IconButton } from '@mui/material'
import Tooltip from '@mui/material/Tooltip'
import {
  type ChangeEvent,
  type MouseEvent,
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react'
import { useTranslation } from 'react-i18next'

import matchCaseIcon from '@/assets/image/component/match_case.svg?react'
import matchWholeWordIcon from '@/assets/image/component/match_whole_word.svg?react'
import UseRegularExpressionIcon from '@/assets/image/component/use_regular_expression.svg?react'
import { buildRegex, compileStringMatcher } from '@/utils/search-matcher'

export type SearchState = {
  text: string
  matchCase: boolean
  matchWholeWord: boolean
  useRegularExpression: boolean
}

type SearchProps = {
  defaultValue?: string
  inputRef?: RefObject<HTMLInputElement | null>
  matchCase?: boolean
  matchWholeWord?: boolean
  useRegularExpression?: boolean
  placeholder?: string
  onSearch: (match: (content: string) => boolean, state: SearchState) => void
  onClick?: (e: MouseEvent<HTMLDivElement>) => void
}

const StyledTextField = styled(TextField)(({ theme }) => ({
  '& .MuiInputBase-root': {
    background: theme.palette.mode === 'light' ? '#fff' : undefined,
    paddingRight: '4px',
  },
  "& .MuiInputBase-root button[aria-pressed='true'] svg path": {
    fill: theme.palette.primary.light,
  },
  "& .MuiInputBase-root button[aria-pressed='false'] svg path": {
    fill: '#A7A7A7',
  },
}))

export const BaseSearchBox = ({
  defaultValue,
  inputRef,
  placeholder,
  matchCase: defaultMatchCase = false,
  matchWholeWord: defaultMatchWholeWord = false,
  useRegularExpression: defaultUseRegularExpression = false,
  onSearch,
  onClick,
}: SearchProps) => {
  const { t } = useTranslation()
  const onSearchRef = useRef(onSearch)

  const lastSearchStateRef = useRef<SearchState>({
    text: defaultValue ?? '',
    matchCase: defaultMatchCase,
    matchWholeWord: defaultMatchWholeWord,
    useRegularExpression: defaultUseRegularExpression,
  })

  const [text, setText] = useState(defaultValue ?? '')
  const [matchCase, setMatchCase] = useState(defaultMatchCase)
  const [matchWholeWord, setMatchWholeWord] = useState(defaultMatchWholeWord)
  const [useRegularExpression, setUseRegularExpression] = useState(
    defaultUseRegularExpression,
  )

  const iconStyle = {
    style: {
      height: '24px',
      width: '24px',
      cursor: 'pointer',
    } as React.CSSProperties,
    inheritViewBox: true,
  }

  useEffect(() => {
    onSearchRef.current = onSearch
  }, [onSearch])

  const emitSearch = useCallback((nextState: SearchState) => {
    const prevState = lastSearchStateRef.current
    const isSameState =
      prevState.text === nextState.text &&
      prevState.matchCase === nextState.matchCase &&
      prevState.matchWholeWord === nextState.matchWholeWord &&
      prevState.useRegularExpression === nextState.useRegularExpression
    if (isSameState) return

    const compiled = compileStringMatcher(nextState.text, nextState)
    onSearchRef.current(compiled.matcher, nextState)

    lastSearchStateRef.current = nextState
  }, [])

  const effectiveErrorMessage = useMemo(() => {
    if (!useRegularExpression || !text) return ''
    const flags = matchCase ? '' : 'i'
    return buildRegex(text, flags) ? '' : t('shared.validation.invalidRegex')
  }, [matchCase, t, text, useRegularExpression])

  const handleChangeText = (
    e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement>,
  ) => {
    const nextText = e.target?.value ?? ''
    setText(nextText)
    emitSearch({
      text: nextText,
      matchCase,
      matchWholeWord,
      useRegularExpression,
    })
  }

  const handleToggleUseRegularExpression = () => {
    const next = !useRegularExpression
    setUseRegularExpression(next)
    emitSearch({
      text,
      matchCase,
      matchWholeWord,
      useRegularExpression: next,
    })
  }

  const handleClearInput = () => {
    setText('')
    emitSearch({ text: '', matchCase, matchWholeWord, useRegularExpression })
  }

  const handleToggleMatchCase = () => {
    const next = !matchCase
    setMatchCase(next)
    emitSearch({ text, matchCase: next, matchWholeWord, useRegularExpression })
  }

  const handleToggleMatchWholeWord = () => {
    const next = !matchWholeWord
    setMatchWholeWord(next)
    emitSearch({ text, matchCase, matchWholeWord: next, useRegularExpression })
  }

  return (
    <Tooltip title={effectiveErrorMessage || ''} placement="bottom-start">
      <StyledTextField
        autoComplete="new-password"
        hiddenLabel
        fullWidth
        size="small"
        variant="outlined"
        inputRef={inputRef}
        spellCheck="false"
        placeholder={placeholder ?? t('shared.placeholders.filter')}
        sx={{ input: { py: 0.65, px: 1.25 } }}
        value={text}
        onClick={onClick}
        onChange={handleChangeText}
        error={!!effectiveErrorMessage}
        slotProps={{
          input: {
            sx: { pr: 1 },
            endAdornment: (
              <Box sx={{ display: 'flex' }}>
                {!!text && (
                  <Tooltip title={t('shared.placeholders.resetInput')}>
                    <IconButton
                      size="small"
                      {...iconStyle}
                      onClick={handleClearInput}
                    >
                      <ClearRounded fontSize="inherit" />
                    </IconButton>
                  </Tooltip>
                )}
                <Tooltip title={t('shared.placeholders.matchCase')}>
                  <IconButton
                    size="small"
                    sx={{ p: 0 }}
                    aria-label={t('shared.placeholders.matchCase')}
                    aria-pressed={matchCase}
                    onClick={handleToggleMatchCase}
                  >
                    <SvgIcon component={matchCaseIcon} {...iconStyle} />
                  </IconButton>
                </Tooltip>
                <Tooltip title={t('shared.placeholders.matchWholeWord')}>
                  <IconButton
                    size="small"
                    sx={{ p: 0 }}
                    aria-label={t('shared.placeholders.matchWholeWord')}
                    aria-pressed={matchWholeWord}
                    onClick={handleToggleMatchWholeWord}
                  >
                    <SvgIcon component={matchWholeWordIcon} {...iconStyle} />
                  </IconButton>
                </Tooltip>
                <Tooltip title={t('shared.placeholders.useRegex')}>
                  <IconButton
                    size="small"
                    sx={{ p: 0 }}
                    aria-label={t('shared.placeholders.useRegex')}
                    aria-pressed={useRegularExpression}
                    onClick={handleToggleUseRegularExpression}
                  >
                    <SvgIcon
                      component={UseRegularExpressionIcon}
                      {...iconStyle}
                    />
                  </IconButton>
                </Tooltip>
              </Box>
            ),
          },
        }}
      />
    </Tooltip>
  )
}
