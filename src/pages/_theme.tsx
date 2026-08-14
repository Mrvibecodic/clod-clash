import { getLuminance, lighten, type PaletteMode } from '@mui/material'

import getSystem from '@/utils/get-system'
const OS = getSystem()

// clod:design-v3 — единая шкала оформления. Раньше эти числа жили россыпью по
// компонентам: тринадцать радиусов, двадцать одна альфа и пять длительностей на
// одни и те же роли. Правим здесь — меняется везде.
export const SHAPE = {
  /** Поверхность: карточка, панель, плитка, строка узла. */
  surface: '14px',
  /** Управление внутри поверхности: кнопка, иконочная плитка, строка списка. */
  control: '10px',
  /** Мелочь: бейдж, тип-чип, поле ввода в панели инструментов. */
  chip: '8px',
  /** Слой поверх содержимого: диалог, шторка. */
  overlay: '16px',
} as const

/** Заливки акцентом: две ступени вместо шести неразличимых. */
export const TINT = {
  /** Наведение и фон под текстом. */
  weak: 0.07,
  /** Иконочная плитка, активный чип, выделенная строка. */
  base: 0.13,
  /** Граница чипа и кольцо выделения. */
  edge: 0.32,
} as const

/** Заголовок карточки: один стиль на все карточки приложения. */
export const CARD_TITLE = {
  fontSize: 12,
  fontWeight: 600,
  letterSpacing: '0.5px',
  textTransform: 'uppercase',
} as const

/** Живое число (трафик, скорость, срок, задержка). */
export const CARD_VALUE = {
  fontSize: 17,
  fontWeight: 700,
  fontVariantNumeric: 'tabular-nums',
} as const

// clod:branding — the default palette is taken 1:1 from the design mockups
// (MOCKUPS-2026-07-30-v2): blue accent, cool gray canvas, white panels in
// light mode and deep slate panels in dark. Providers can repaint everything
// through the theme settings; ACCENT_PRESETS below are the five accents the
// mockups shipped with.
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

// dark mode
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

/** The five accent choices from the mockups, offered as one-click presets. */
export const ACCENT_PRESETS = [
  '#2E7CF6',
  '#14B8A6',
  '#8B5CF6',
  '#22C55E',
  '#F97316',
] as const

/**
 * Акцент под тему.
 *
 * Пресеты и цвет провайдера подбираются на светлом фоне и на тёмном глохнут:
 * синий и фиолетовый уходят в фон панели, и цифры скорости с чипами перестают
 * читаться. В тёмной теме поднимаем светлоту ровно настолько, чтобы вернуть
 * контраст, и не трогаем те цвета, которым это не нужно (зелёный, бирюзовый).
 */
export const accentForMode = (color: string, mode: PaletteMode) => {
  if (mode !== 'dark') return color
  try {
    const luminance = getLuminance(color)
    if (luminance >= 0.32) return color
    return lighten(color, Math.min(0.42, (0.32 - luminance) * 1.35))
  } catch {
    // Цвет мог прийти из настроек провайдера в неразобранном виде.
    return color
  }
}
