import { alpha, type Theme } from '@mui/material'

export const proxyTileSx =
  (clickable: boolean, showDelay: boolean) =>
  ({ palette: { primary, text } }: Theme) => {
    const tileBg = alpha(text.primary, 0.07)
    const selectedBg = alpha(primary.main, 0.14)

    return {
      backgroundColor: tileBg,
      transition: 'background-color 150ms cubic-bezier(0.2, 0, 0, 1)',
      '&:hover': {
        backgroundColor: clickable ? alpha(text.primary, 0.1) : tileBg,
      },
      '&.Mui-selected': {
        backgroundColor: selectedBg,
        boxShadow: `inset 3px 0 0 ${primary.main}`,
      },
      '&.Mui-selected:hover': {
        backgroundColor: clickable ? alpha(primary.main, 0.2) : selectedBg,
      },
      '&.Mui-focusVisible': {
        outline: `2px solid ${alpha(primary.main, 0.6)}`,
        outlineOffset: -2,
      },
      '&:hover .the-check': { display: !showDelay ? 'block' : 'none' },
      '&:hover .the-delay': { display: showDelay ? 'flex' : 'none' },
      '&:hover .the-icon': { display: 'none' },
      ...(!clickable && { cursor: 'default' }),
    }
  }

export const PING_SX = {
  display: 'flex',
  alignItems: 'baseline',
  fontWeight: 600,
  fontVariantNumeric: 'tabular-nums',
} as const
