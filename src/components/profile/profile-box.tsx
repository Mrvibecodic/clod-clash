import { alpha, Box, styled } from '@mui/material'

import { SHAPE } from '@/pages/_theme'

export const ProfileBox = styled(Box, {
  shouldForwardProp: (prop) => prop !== 'dimmed',
})<{ dimmed?: boolean }>(({ theme, 'aria-selected': selected, dimmed }) => {
  const { mode, primary, text } = theme.palette
  const isSelected = !!selected

  const paper = theme.palette.background.paper
  const backgroundColor = isSelected
    ? `color-mix(in srgb, ${primary.main} 7%, ${paper})`
    : paper

  const color = mode === 'light' ? text.secondary : alpha(text.secondary, 0.65)
  const h2color = isSelected ? primary.main : text.primary

  return {
    position: 'relative',
    display: 'block',
    cursor: 'pointer',
    textAlign: 'left',
    padding: '12px 16px 14px',
    boxSizing: 'border-box',
    width: '100%',
    backgroundColor,
    border: isSelected
      ? `1.5px solid ${primary.main}`
      : `1.5px solid var(--card-line)`,
    boxShadow: isSelected
      ? `0 0 0 3px ${alpha(primary.main, mode === 'light' ? 0.22 : 0.28)}`
      : 'var(--card-shadow)',
    borderRadius: SHAPE.surface,
    color,
    opacity: dimmed && !isSelected ? 0.55 : 1,
    filter: dimmed && !isSelected ? 'grayscale(0.55)' : 'none',
    transition: theme.transitions.create(
      ['background-color', 'border-color', 'box-shadow', 'opacity'],
      { duration: theme.transitions.duration.short },
    ),
    '& h2': { color: h2color },
  }
})
