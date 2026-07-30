import getSystem from '@/utils/get-system'
const OS = getSystem()

// clod:branding — the default palette follows the app icon: indigo-violet
// accents instead of the upstream iOS blue, so a fresh install looks like
// Clod Clash and not like the project it was forked from. Providers can
// still repaint everything through the theme settings.
export const defaultTheme = {
  primary_color: '#5B4FD6',
  secondary_color: '#9333EA',
  primary_text: '#1D1B2E',
  secondary_text: '#3C3C4399',
  info_color: '#5B4FD6',
  error_color: '#E5484D',
  warning_color: '#F76B15',
  success_color: '#189A4A',
  background_color: '#F6F5FA',
  font_family: `-apple-system, BlinkMacSystemFont,"Microsoft YaHei UI", "Microsoft YaHei", Roboto, "Helvetica Neue", Arial, sans-serif, "Apple Color Emoji"${
    OS === 'windows' ? ', twemoji mozilla' : ''
  }`,
}

// dark mode
export const defaultDarkTheme = {
  ...defaultTheme,
  primary_color: '#8B7CF6',
  secondary_color: '#C084FC',
  primary_text: '#F4F2FF',
  background_color: '#232135',
  secondary_text: '#EBEBF599',
  info_color: '#8B7CF6',
  error_color: '#FF6369',
  warning_color: '#FFA057',
  success_color: '#3DD68C',
}
