import getSystem from '@/utils/get-system'
const OS = getSystem()

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
