import {
  alpha,
  createTheme,
  lighten,
  Theme as MuiTheme,
  Shadows,
} from '@mui/material'
import {
  getCurrentWebviewWindow,
  WebviewWindow,
} from '@tauri-apps/api/webviewWindow'
import { Theme as TauriOsTheme } from '@tauri-apps/api/window'
import { useEffect, useMemo, useRef } from 'react'

import { useVerge } from '@/hooks/use-verge'
import { accentForMode, defaultDarkTheme, defaultTheme } from '@/pages/_theme'
import { useSetThemeMode, useThemeMode } from '@/services/states'

const CSS_INJECTION_SCOPE_ROOT = '[data-css-injection-root]'
const CSS_INJECTION_SCOPE_LIMIT =
  ':is(.monaco-editor .view-lines, .monaco-editor .view-line, .monaco-editor .margin, .monaco-editor .margin-view-overlays, .monaco-editor .view-overlays, .monaco-editor [class^="mtk"], .monaco-editor [class*=" mtk"])'
const TOP_LEVEL_AT_RULES = [
  '@charset',
  '@import',
  '@namespace',
  '@font-face',
  '@keyframes',
  '@counter-style',
  '@page',
  '@property',
  '@font-feature-values',
  '@color-profile',
]
let cssScopeSupport: boolean | null = null

const THEME_FADE_MS = 380

const buildShadows = (mode: 'light' | 'dark'): Shadows => {
  const soft =
    mode === 'light'
      ? '0 1px 2px rgba(15, 22, 38, 0.06), 0 1px 3px rgba(15, 22, 38, 0.08)'
      : '0 1px 2px rgba(0, 0, 0, 0.36), 0 1px 3px rgba(0, 0, 0, 0.32)'
  const raised =
    mode === 'light'
      ? '0 6px 16px rgba(15, 22, 38, 0.12)'
      : '0 8px 20px rgba(0, 0, 0, 0.48)'
  const floating =
    mode === 'light'
      ? '0 12px 32px rgba(15, 22, 38, 0.16)'
      : '0 14px 36px rgba(0, 0, 0, 0.56)'
  const overlay =
    mode === 'light'
      ? '0 24px 60px rgba(15, 22, 38, 0.24)'
      : '0 26px 64px rgba(0, 0, 0, 0.64)'

  return Array.from({ length: 25 }, (_, level) => {
    if (level === 0) return 'none'
    if (level <= 4) return soft
    if (level <= 8) return raised
    if (level <= 16) return floating
    return overlay
  }) as unknown as Shadows
}

const cardSurfaceVars = (mode: 'light' | 'dark') =>
  mode === 'light'
    ? {
        line: 'rgba(17, 24, 39, 0.06)',
        shadow:
          '0 1px 2px rgba(15, 22, 38, 0.05), 0 6px 18px rgba(15, 22, 38, 0.06)',
        shadowHover:
          '0 2px 4px rgba(15, 22, 38, 0.06), 0 12px 28px rgba(15, 22, 38, 0.12)',
      }
    : {
        line: 'rgba(255, 255, 255, 0.06)',
        shadow:
          '0 1px 2px rgba(0, 0, 0, 0.45), 0 8px 22px rgba(0, 0, 0, 0.28)',
        shadowHover:
          '0 2px 4px rgba(0, 0, 0, 0.5), 0 12px 30px rgba(0, 0, 0, 0.5)',
      }

const canUseCssScope = () => {
  if (cssScopeSupport !== null) {
    return cssScopeSupport
  }
  try {
    const testStyle = document.createElement('style')
    testStyle.textContent = '@scope (:root) { }'
    document.head.appendChild(testStyle)
    cssScopeSupport = !!testStyle.sheet?.cssRules?.length
    document.head.removeChild(testStyle)
  } catch {
    cssScopeSupport = false
  }
  return cssScopeSupport
}

const wrapCssInjectionWithScope = (css?: string) => {
  if (!css?.trim()) {
    return ''
  }
  const lowerCss = css.toLowerCase()
  const hasTopLevelOnlyRule = TOP_LEVEL_AT_RULES.some((rule) =>
    lowerCss.includes(rule),
  )
  if (hasTopLevelOnlyRule) {
    return null
  }
  const scopeRoot = CSS_INJECTION_SCOPE_ROOT
  const scopeLimit = CSS_INJECTION_SCOPE_LIMIT
  const scopedBlock = `@scope (${scopeRoot}) to (${scopeLimit}) {
${css}
}`
  return scopedBlock
}

export const useCustomTheme = () => {
  const appWindow: WebviewWindow = useMemo(() => getCurrentWebviewWindow(), [])
  const { verge } = useVerge()
  const { theme_mode, theme_setting } = verge ?? {}
  const mode = useThemeMode()
  const setMode = useSetThemeMode()
  const userBackgroundImage = theme_setting?.background_image || ''
  const hasUserBackground = !!userBackgroundImage

  useEffect(() => {
    if (theme_mode === 'light' || theme_mode === 'dark') {
      setMode(theme_mode)
    }
  }, [theme_mode, setMode])

  useEffect(() => {
    if (theme_mode !== 'system') {
      return
    }

    let isMounted = true

    const timerId = setTimeout(() => {
      if (!isMounted) return
      appWindow
        .theme()
        .then((systemTheme) => {
          if (isMounted && systemTheme) {
            setMode(systemTheme)
          }
        })
        .catch((err) => {
          console.error('Failed to get initial system theme:', err)
        })
    }, 0)

    const unlistenPromise = appWindow.onThemeChanged(({ payload }) => {
      if (isMounted) {
        setMode(payload)
      }
    })

    return () => {
      isMounted = false
      clearTimeout(timerId)
      unlistenPromise
        .then((unlistenFn) => {
          if (typeof unlistenFn === 'function') {
            unlistenFn()
          }
        })
        .catch((err) => {
          console.error('Failed to unlisten from theme changes:', err)
        })
    }
  }, [theme_mode, appWindow, setMode])

  useEffect(() => {
    if (theme_mode === undefined) {
      return
    }

    if (theme_mode === 'system') {
      appWindow.setTheme(null).catch((err) => {
        console.error(
          'Failed to set window theme to follow system (setTheme(null)):',
          err,
        )
      })
    } else if (mode) {
      appWindow.setTheme(mode as TauriOsTheme).catch((err) => {
        console.error(`Failed to set window theme to ${mode}:`, err)
      })
    }
  }, [mode, appWindow, theme_mode])

  const theme = useMemo(() => {
    const setting = theme_setting || {}
    const dt = mode === 'light' ? defaultTheme : defaultDarkTheme
    let muiTheme: MuiTheme

    const shared = {
      breakpoints: {
        values: { xs: 0, sm: 650, md: 900, lg: 1200, xl: 1536 },
      },
      shadows: buildShadows(mode),
      transitions: {
        easing: {
          easeInOut: 'cubic-bezier(0.2, 0, 0, 1)',
          easeOut: 'cubic-bezier(0.2, 0, 0, 1)',
          easeIn: 'cubic-bezier(0.4, 0, 1, 1)',
          sharp: 'cubic-bezier(0.4, 0, 0.6, 1)',
        },
        duration: {
          shortest: 120,
          shorter: 150,
          short: 180,
          standard: 260,
          complex: 320,
          enteringScreen: 260,
          leavingScreen: 200,
        },
      },
      components: {
        MuiButton: {
          styleOverrides: {
            root: {
              textTransform: 'none',
              borderRadius: 999,
              fontWeight: 600,
            },
          },
        },
      },
    } as const

    const divider =
      mode === 'light' ? 'rgba(17, 24, 39, 0.11)' : 'rgba(255, 255, 255, 0.11)'

    try {
      muiTheme = createTheme({
        ...shared,
        palette: {
          mode,
          primary: {
            main: accentForMode(
              setting.primary_color || dt.primary_color,
              mode,
            ),
          },
          secondary: {
            main: accentForMode(
              setting.secondary_color || dt.secondary_color,
              mode,
            ),
          },
          info: {
            main: accentForMode(setting.info_color || dt.info_color, mode),
          },
          error: { main: setting.error_color || dt.error_color },
          warning: { main: setting.warning_color || dt.warning_color },
          success: { main: setting.success_color || dt.success_color },
          divider,
          text: {
            primary: setting.primary_text || dt.primary_text,
            secondary: setting.secondary_text || dt.secondary_text,
          },
          background: {
            paper:
              mode === 'light'
                ? '#FFFFFF'
                : lighten(dt.background_color, 0.045),
            default: dt.background_color,
          },
        },
        typography: {
          fontFamily: setting.font_family
            ? `${setting.font_family}, ${dt.font_family}`
            : dt.font_family,
        },
      })
    } catch (e) {
      console.error('Error creating MUI theme, falling back to defaults:', e)
      muiTheme = createTheme({
        ...shared,
        palette: {
          mode,
          primary: { main: accentForMode(dt.primary_color, mode) },
          secondary: { main: accentForMode(dt.secondary_color, mode) },
          info: { main: accentForMode(dt.info_color, mode) },
          error: { main: dt.error_color },
          warning: { main: dt.warning_color },
          success: { main: dt.success_color },
          divider,
          text: { primary: dt.primary_text, secondary: dt.secondary_text },
          background: {
            paper:
              mode === 'light'
                ? '#FFFFFF'
                : lighten(dt.background_color, 0.045),
            default: dt.background_color,
          },
        },
        typography: { fontFamily: dt.font_family },
      })
    }

    const rootEle = document.documentElement
    if (rootEle) {
      const backgroundColor = mode === 'light' ? '#ECECEC' : dt.background_color
      const selectColor = mode === 'light' ? '#f5f5f5' : '#3E3E3E'
      const scrollColor = mode === 'light' ? '#90939980' : '#555555'
      const card = cardSurfaceVars(mode)
      rootEle.style.setProperty('--divider-color', muiTheme.palette.divider)
      rootEle.style.setProperty('--background-color', backgroundColor)
      rootEle.style.setProperty('--selection-color', selectColor)
      rootEle.style.setProperty('--scroller-color', scrollColor)
      rootEle.style.setProperty('--primary-main', muiTheme.palette.primary.main)
      rootEle.style.setProperty(
        '--background-color-alpha',
        alpha(muiTheme.palette.primary.main, 0.1),
      )
      rootEle.style.setProperty('--card-line', card.line)
      rootEle.style.setProperty('--card-shadow', card.shadow)
      rootEle.style.setProperty('--card-shadow-hover', card.shadowHover)
      rootEle.style.setProperty(
        '--canvas-gradient',
        hasUserBackground
          ? 'none'
          : `linear-gradient(180deg, ${alpha(
              muiTheme.palette.primary.main,
              mode === 'light' ? 0.05 : 0.07,
            )} 0px, ${alpha(muiTheme.palette.primary.main, 0)} 240px)`,
      )
      rootEle.style.setProperty(
        '--window-border-color',
        mode === 'light' ? '#cccccc' : '#1E1E1E',
      )
      rootEle.style.setProperty(
        '--scrollbar-bg',
        mode === 'light' ? '#f1f1f1' : '#2E303D',
      )
      rootEle.style.setProperty(
        '--scrollbar-thumb',
        mode === 'light' ? '#c1c1c1' : '#555555',
      )
      rootEle.style.setProperty(
        '--user-background-image',
        hasUserBackground ? `url('${userBackgroundImage}')` : 'none',
      )
      rootEle.style.setProperty(
        '--background-blend-mode',
        setting.background_blend_mode || 'normal',
      )
      rootEle.style.setProperty(
        '--background-opacity',
        setting.background_opacity !== undefined
          ? String(setting.background_opacity)
          : '1',
      )
      rootEle.setAttribute('data-css-injection-root', 'true')
    }

    let styleElement = document.querySelector('style#verge-theme')
    if (!styleElement) {
      styleElement = document.createElement('style')
      styleElement.id = 'verge-theme'
      document.head.appendChild(styleElement!)
    }

    if (styleElement) {
      let scopedCss: string | null = null
      if (canUseCssScope() && setting.css_injection) {
        scopedCss = wrapCssInjectionWithScope(setting.css_injection)
      }
      const effectiveInjectedCss = scopedCss ?? setting.css_injection ?? ''
      const globalStyles = `
        ::-webkit-scrollbar {
          width: 8px;
          height: 8px;
          background-color: var(--scrollbar-bg);
        }
        ::-webkit-scrollbar-thumb {
          background-color: var(--scrollbar-thumb);
          border-radius: 4px;
        }
        ::-webkit-scrollbar-thumb:hover {
          background-color: ${mode === 'light' ? '#a1a1a1' : '#666666'};
        }

        body {
          background-color: var(--background-color);
          ${
            hasUserBackground
              ? `
            background-image: var(--user-background-image);
            background-size: cover;
            background-position: center;
            background-attachment: fixed;
            background-blend-mode: var(--background-blend-mode);
            opacity: var(--background-opacity);
          `
              : ''
          }
        }

        .MuiDialog-paper,
        .MuiDrawer-paper,
        .MuiPopover-paper {
          border-color: var(--window-border-color);
        }

        .MuiDialog-paper {
          background-color: ${muiTheme.palette.background.paper};
          border-radius: 16px;
        }

        :focus {
          outline: none;
        }
        :focus-visible {
          outline: 2px solid ${muiTheme.palette.primary.main};
          outline-offset: 2px;
        }
        [data-tauri-drag-region] {
          outline: none;
        }

        .clod-theme-fade,
        .clod-theme-fade *:not(svg):not(path) {
          transition:
            background-color ${THEME_FADE_MS}ms ${muiTheme.transitions.easing.easeInOut},
            border-color ${THEME_FADE_MS}ms ${muiTheme.transitions.easing.easeInOut},
            color ${THEME_FADE_MS}ms ${muiTheme.transitions.easing.easeInOut} !important;
        }

        @media (prefers-reduced-motion: reduce) {
          .clod-theme-fade,
          .clod-theme-fade * {
            transition: none !important;
          }
        }
      `

      styleElement.innerHTML = effectiveInjectedCss + globalStyles
    }

    return muiTheme
  }, [mode, theme_setting, userBackgroundImage, hasUserBackground])

  const previousModeRef = useRef<string | undefined>(undefined)
  useEffect(() => {
    const root = document.documentElement
    if (
      previousModeRef.current === undefined ||
      previousModeRef.current === mode
    ) {
      previousModeRef.current = mode
      return
    }
    previousModeRef.current = mode
    root.classList.add('clod-theme-fade')
    const id = setTimeout(
      () => root.classList.remove('clod-theme-fade'),
      THEME_FADE_MS,
    )
    return () => {
      clearTimeout(id)
      root.classList.remove('clod-theme-fade')
    }
  }, [mode])

  useEffect(() => {
    const id = setTimeout(() => {
      const dom = document.querySelector('#Gradient2')
      if (dom) {
        dom.innerHTML = `
        <stop offset="0%" stop-color="${theme.palette.primary.main}" />
        <stop offset="80%" stop-color="${theme.palette.primary.dark}" />
        <stop offset="100%" stop-color="${theme.palette.primary.dark}" />
        `
      }
    }, 0)
    return () => clearTimeout(id)
  }, [theme.palette.primary.main, theme.palette.primary.dark])

  return { theme }
}
