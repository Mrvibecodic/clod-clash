import { alpha, styled, Box } from '@mui/material'
import type { ReactNode } from 'react'

// clod:design-v3 — лог читают глазами по столбцу: время и сообщение
// моноширинным, уровень — чипом, а не просто цветным словом (радиус 2 без
// фона не рисовал ничего). Разделитель теперь во всю строку.
const MONO =
  'ui-monospace, "Cascadia Mono", "Segoe UI Mono", "Roboto Mono", Consolas, monospace'

const Item = styled(Box)(({ theme }) => {
  const { palette, transitions } = theme
  return {
    padding: '8px 12px',
    lineHeight: 1.4,
    borderBottom: `1px solid ${palette.divider}`,
    fontSize: '0.875rem',
    fontFamily: MONO,
    userSelect: 'text',
    transition: transitions.create(['background-color'], {
      duration: transitions.duration.short,
    }),
    '&:hover': { backgroundColor: palette.action.hover },
    '& .time': {
      color: palette.text.secondary,
      fontVariantNumeric: 'tabular-nums',
    },
    '& .type': {
      display: 'inline-block',
      marginLeft: 8,
      padding: '1px 7px',
      textAlign: 'center',
      borderRadius: 999,
      textTransform: 'uppercase',
      fontWeight: '600',
      fontSize: 11,
      letterSpacing: '0.3px',
      fontFamily: theme.typography.fontFamily,
      backgroundColor: alpha(palette.text.primary, 0.08),
    },
    '& .type[data-type="error"], & .type[data-type="err"]': {
      color: palette.error.main,
      backgroundColor: alpha(palette.error.main, 0.13),
    },
    '& .type[data-type="warning"], & .type[data-type="warn"]': {
      color: palette.warning.main,
      backgroundColor: alpha(palette.warning.main, 0.13),
    },
    '& .type[data-type="info"], & .type[data-type="inf"]': {
      color: palette.info.main,
      backgroundColor: alpha(palette.info.main, 0.13),
    },
    '& .data': {
      color: palette.text.primary,
      overflowWrap: 'anywhere',
    },
    '& .highlight': {
      backgroundColor: palette.mode === 'dark' ? '#ffeb3b40' : '#ffeb3b90',
      borderRadius: 4,
      padding: '0 2px',
    },
  }
})

interface Props {
  value: ILogItem
  /** Регекс подсветки, общий для всего списка (см. `buildHighlightRegex`). */
  highlight?: RegExp | null
}

const LogItem = ({ value, highlight }: Props) => {
  const renderHighlightText = (text: string) => {
    if (!highlight) return text

    try {
      const elements: ReactNode[] = []
      let lastIndex = 0

      // `matchAll` идёт по копии регекса: общий для всех строк не сдвигается
      for (const match of text.matchAll(highlight)) {
        const start = match.index
        const matchText = match[0]

        if (matchText === '') continue

        if (start > lastIndex) {
          elements.push(text.slice(lastIndex, start))
        }

        elements.push(
          <span key={`highlight-${start}`} className="highlight">
            {matchText}
          </span>,
        )

        lastIndex = start + matchText.length
      }

      if (lastIndex < text.length) {
        elements.push(text.slice(lastIndex))
      }

      return elements.length ? elements : text
    } catch {
      return text
    }
  }

  return (
    <Item>
      <div>
        <span className="time">{renderHighlightText(value.time || '')}</span>
        <span className="type" data-type={value.type.toLowerCase()}>
          {renderHighlightText(value.type)}
        </span>
      </div>
      <div>
        <span className="data">{renderHighlightText(value.payload)}</span>
      </div>
    </Item>
  )
}

export default LogItem
