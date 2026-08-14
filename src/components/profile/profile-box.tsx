import { alpha, Box, styled } from '@mui/material'

import { SHAPE } from '@/pages/_theme'

/**
 * clod: активная подписка раньше отличалась только полоской слева и цветом
 * заголовка — на сетке из шести карточек этого не видно. Теперь она залита
 * акцентом и обведена им же, а истёкшая приглушается: состояние карточки
 * должно читаться с одного взгляда, до чтения дат.
 */
export const ProfileBox = styled(Box, {
  shouldForwardProp: (prop) => prop !== 'dimmed',
})<{ dimmed?: boolean }>(({ theme, 'aria-selected': selected, dimmed }) => {
  const { mode, primary, text } = theme.palette
  const isSelected = !!selected

  // clod:design-v3 — панель одна на всё приложение: раньше карточка была
  // прибита к #282A36 и в тёмной теме отличалась от соседних поверхностей.
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
    // clod:card-v2 — карточка стала просторнее: строки больше не жались в
    // фиксированные 26 px и не наезжали друг на друга.
    padding: '12px 16px 14px',
    boxSizing: 'border-box',
    width: '100%',
    backgroundColor,
    border: isSelected
      ? `1.5px solid ${primary.main}`
      : `1.5px solid ${theme.palette.divider}`,
    // Кольцо было и раньше, но его стирало глобальное правило
    // `box-shadow: none !important` — теперь оно действительно видно.
    boxShadow: isSelected
      ? `0 0 0 3px ${alpha(primary.main, mode === 'light' ? 0.22 : 0.28)}`
      : 'none',
    borderRadius: SHAPE.surface,
    color,
    // Приглушение — только для истёкших и никогда для активной: подписка, на
    // которой человек сидит, обязана оставаться читаемой.
    opacity: dimmed && !isSelected ? 0.55 : 1,
    filter: dimmed && !isSelected ? 'grayscale(0.55)' : 'none',
    transition: theme.transitions.create(
      ['background-color', 'border-color', 'box-shadow', 'opacity'],
      { duration: theme.transitions.duration.short },
    ),
    '& h2': { color: h2color },
  }
})
