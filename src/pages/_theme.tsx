import { getLuminance, lighten, type PaletteMode } from '@mui/material'

import getSystem from '@/utils/get-system'
const OS = getSystem()

export const SHAPE = {
  surface: '14px',
  control: '10px',
  chip: '8px',
  overlay: '16px',
} as const

export const TINT = {
  weak: 0.07,
  base: 0.13,
  edge: 0.32,
} as const

export const CARD_TITLE = {
  fontSize: 12.5,
  fontWeight: 600,
  letterSpacing: '0.2px',
  textTransform: 'none',
} as const

export const CARD_VALUE = {
  fontSize: 17,
  fontWeight: 700,
  fontVariantNumeric: 'tabular-nums',
} as const

export const CARD_SURFACE = {
  bgcolor: 'background.paper',
  borderRadius: SHAPE.surface,
  border: '1px solid var(--card-line)',
  boxShadow: 'var(--card-shadow)',
} as const

const LIFT_EASING = 'cubic-bezier(0.2, 0, 0, 1)'

export const CARD_LIFT = {
  transition: `transform 180ms ${LIFT_EASING}, box-shadow 180ms ${LIFT_EASING}, border-color 180ms ${LIFT_EASING}, background-color 180ms ${LIFT_EASING}`,
  '&:hover': {
    transform: 'translateY(-2px)',
    boxShadow: 'var(--card-shadow-hover)',
  },
  '@media (prefers-reduced-motion: reduce)': {
    '&:hover': { transform: 'none' },
  },
} as const

export const defaultTheme = {
  primary_color: '#2E7CF6',
  secondary_color: '#D97706',
  primary_text: '#1A2130',
  secondary_text: '#5A6478',
  info_color: '#2E7CF6',
  error_color: '#EF4444',
  warning_color: '#F59E0B',
  success_color: '#22C55E',
  background_color: '#F1F4F9',
  font_family: `-apple-system, BlinkMacSystemFont,"Microsoft YaHei UI", "Microsoft YaHei", Roboto, "Helvetica Neue", Arial, sans-serif, "Apple Color Emoji"${
    OS === 'windows' ? ', twemoji mozilla' : ''
  }`,
}

export const defaultDarkTheme = {
  ...defaultTheme,
  primary_color: '#4287F5',
  secondary_color: '#EA580C',
  primary_text: '#EBEEF5',
  background_color: '#0E1116',
  secondary_text: '#9AA3B5',
  info_color: '#4287F5',
  error_color: '#F87171',
  warning_color: '#FBBF24',
  success_color: '#4ADE80',
}

export const ACCENT_PRESETS = [
  '#2E7CF6',
  '#14B8A6',
  '#8B5CF6',
  '#22C55E',
  '#F97316',
] as const

export const accentForMode = (color: string, mode: PaletteMode) => {
  if (mode !== 'dark') return color
  try {
    const luminance = getLuminance(color)
    if (luminance >= 0.32) return color
    return lighten(color, Math.min(0.42, (0.32 - luminance) * 1.35))
  } catch {
    return color
  }
}
