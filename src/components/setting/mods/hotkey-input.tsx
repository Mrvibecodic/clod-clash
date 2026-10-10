import { CloseRounded } from '@mui/icons-material'
import { alpha, Box, IconButton, styled } from '@mui/material'
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { MONO_TEXT } from '@/components/base/base-mono'
import { parseHotkey } from '@/utils/parse-hotkey'

const KeyWrapper = styled('div')(({ theme }) => ({
  position: 'relative',
  width: 200,
  minHeight: 34,

  '> input': {
    position: 'absolute',
    top: 0,
    left: 0,
    width: '100%',
    height: '100%',
    zIndex: 1,
    opacity: 0,
    cursor: 'pointer',
  },
  '> input:focus + .list': {
    borderColor: theme.palette.primary.main,
  },
  '.list': {
    display: 'flex',
    alignItems: 'center',
    flexWrap: 'wrap',
    gap: 4,
    width: '100%',
    height: '100%',
    minHeight: 34,
    boxSizing: 'border-box',
    padding: '3px 10px',
    border: '1px solid',
    borderRadius: 8,
    borderColor: alpha(theme.palette.text.primary, 0.23),
    transition: 'border-color 150ms',
  },
  '> input:hover + .list': {
    borderColor: theme.palette.text.primary,
  },
  '.item': {
    ...MONO_TEXT,
    fontSize: 11.5,
    fontWeight: 600,
    lineHeight: '20px',
    color: theme.palette.text.primary,
    backgroundColor: alpha(theme.palette.text.primary, 0.1),
    borderRadius: 4,
    padding: '0 6px',
  },
  '.delimiter, .empty': {
    fontSize: 12.5,
    color: theme.palette.text.secondary,
  },
}))

interface Props {
  value: string[]
  onChange: (value: string[]) => void
}

export const HotkeyInput = (props: Props) => {
  const { value, onChange } = props
  const { t } = useTranslation()

  const changeRef = useRef<string[]>([])
  const [keys, setKeys] = useState(value)

  return (
    <Box sx={{ display: 'flex', alignItems: 'center', gap: 0.5 }}>
      <KeyWrapper>
        <input
          onKeyUp={() => {
            const ret = changeRef.current.slice()
            if (ret.length) {
              onChange(ret)
              changeRef.current = []
            }
          }}
          onKeyDown={(e) => {
            e.preventDefault()
            e.stopPropagation()

            const key = parseHotkey(e)
            if (key === 'UNIDENTIFIED') return

            changeRef.current = [...new Set([...changeRef.current, key])]
            setKeys(changeRef.current)
          }}
        />

        <div className="list">
          {keys.length ? (
            keys.map((key, index) => (
              <Box
                sx={{ display: 'flex', alignItems: 'center', gap: '4px' }}
                key={key}
              >
                <span className="delimiter" hidden={index === 0}>
                  +
                </span>
                <div className="item">{key}</div>
              </Box>
            ))
          ) : (
            <span className="empty">
              {t('settings.modals.hotkey.messages.notSet')}
            </span>
          )}
        </div>
      </KeyWrapper>

      <IconButton
        size="small"
        title={t('shared.actions.clear')}
        sx={{
          color: 'text.secondary',
          visibility: keys.length ? 'visible' : 'hidden',
        }}
        onClick={() => {
          onChange([])
          setKeys([])
        }}
      >
        <CloseRounded fontSize="small" />
      </IconButton>
    </Box>
  )
}
