import { Box, Paper, ThemeProvider } from '@mui/material'
import dayjs from 'dayjs'
import relativeTime from 'dayjs/plugin/relativeTime'
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { Outlet, useLocation, useNavigate } from 'react-router'

import { BaseErrorBoundary, BaseLoading } from '@/components/base'
import { NoticeManager } from '@/components/layout/notice-manager'
import { UpdateButton } from '@/components/layout/update-button'
import {
  WindowControls,
  WindowResizeHandles,
} from '@/components/layout/window-controller'
import { HwidLimitDialog } from '@/components/profile/hwid-limit-dialog'
import { useI18n } from '@/hooks/use-i18n'
import { useEnforceLockedTargets } from '@/hooks/use-locked-targets'
import { useModeWindowSize } from '@/hooks/use-mode-window-size'
import { useVerge } from '@/hooks/use-verge'
import { useVisibility } from '@/hooks/use-visibility'
import { useWindowDecorations } from '@/hooks/use-window'
import { useThemeMode } from '@/services/states'
import getSystem from '@/utils/get-system'

import {
  useCustomTheme,
  useLayoutEvents,
  useLoadingOverlay,
} from './_layout/hooks'
import { handleNoticeMessage } from './_layout/utils'
import { preloadLogsPage, preloadNavigationRoutes } from './_navigation'

import 'dayjs/locale/ru'
import 'dayjs/locale/zh-cn'

const LogsPage = lazy(() => preloadLogsPage())

dayjs.extend(relativeTime)

const OS = getSystem()

const Layout = () => {
  const mode = useThemeMode()
  const { t } = useTranslation()
  const { theme } = useCustomTheme()
  const { verge } = useVerge()
  const { language } = verge ?? {}
  const { switchLanguage } = useI18n()
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const isLogsPage = pathname === '/logs'
  const pageVisible = useVisibility()
  const themeReady = useMemo(() => Boolean(theme), [theme])

  useModeWindowSize()
  useEnforceLockedTargets()

  const windowControlsRef = useRef<any>(null)
  const { decorated } = useWindowDecorations()

  const customTitlebar = useMemo(
    () =>
      decorated === false ? (
        <div className="the_titlebar">
          <div
            className="the_titlebar-drag-region"
            data-tauri-drag-region="true"
          />
          <WindowControls ref={windowControlsRef} />
        </div>
      ) : null,
    [decorated],
  )

  useLoadingOverlay(themeReady)

  useEffect(() => {
    if (!themeReady || !pageVisible) {
      return
    }

    const controller = new AbortController()
    void preloadNavigationRoutes(controller.signal)

    return () => {
      controller.abort()
    }
  }, [themeReady, pageVisible])

  const handleNotice = useCallback(
    (payload: [string, string]) => {
      const [status, msg] = payload
      try {
        handleNoticeMessage(status, msg, t, navigate)
      } catch (error) {
        console.error('[Обработка уведомлений] Ошибка:', error)
      }
    },
    [t, navigate],
  )

  useLayoutEvents(handleNotice)

  useEffect(() => {
    if (language) {
      dayjs.locale(language === 'zh' ? 'zh-cn' : language)
      switchLanguage(language)
    }
  }, [language, switchLanguage])

  if (!themeReady) {
    return (
      <div
        style={{
          width: '100vw',
          height: '100vh',
          background: mode === 'light' ? '#fff' : '#181a1b',
          transition: 'background 0.2s',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: mode === 'light' ? '#333' : '#fff',
        }}
      ></div>
    )
  }

  return (
    <ThemeProvider theme={theme}>
      <NoticeManager position={verge?.notice_position} />
      <HwidLimitDialog />
      <Box sx={{ display: 'none' }} aria-hidden>
        <UpdateButton />
      </Box>
      <div
        style={{
          animation: 'fadeIn 0.5s',
          WebkitAnimation: 'fadeIn 0.5s',
        }}
      />
      <style>
        {`
            @keyframes fadeIn {
              from { opacity: 0; }
              to { opacity: 1; }
            }
          `}
      </style>
      <Paper
        square
        elevation={0}
        className={`${OS} layout`}
        style={{
          borderTopLeftRadius: '0px',
          borderTopRightRadius: '0px',
        }}
        onContextMenu={(e) => {
          if (OS !== 'windows') return
          const target = e.target as HTMLElement | null
          if (
            target?.closest('input, textarea, [contenteditable="true"]') == null
          ) {
            e.preventDefault()
          }
        }}
        sx={[
          ({ palette }) => ({
            bgcolor: palette.background.default,
            backgroundImage: 'var(--canvas-gradient)',
            backgroundRepeat: 'no-repeat',
          }),
          OS === 'linux'
            ? {
                borderRadius: '8px',
                width: '100vw',
                height: '100vh',
              }
            : {},
        ]}
      >
        {decorated === false && <WindowResizeHandles />}

        {customTitlebar}

        <div className="layout-content">
          <div className="layout-content__right">
            <div className="the-bar"></div>
            <div className="the-content">
              <BaseErrorBoundary>
                <Outlet />
              </BaseErrorBoundary>
              {isLogsPage && (
                <div
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    right: 0,
                    bottom: 0,
                  }}
                >
                  <Suspense
                    fallback={
                      <Box
                        sx={{
                          display: 'flex',
                          height: '100%',
                          alignItems: 'center',
                          justifyContent: 'center',
                        }}
                      >
                        <BaseLoading />
                      </Box>
                    }
                  >
                    <LogsPage />
                  </Suspense>
                </div>
              )}
            </div>
          </div>
        </div>
      </Paper>
    </ThemeProvider>
  )
}

export default Layout
