import { Button } from '@mui/material'
import { ReactNode } from 'react'
import { ErrorBoundary } from 'react-error-boundary'
import { useTranslation } from 'react-i18next'
import { useLocation, useNavigate, useRouteError } from 'react-router'

interface Exit {
  label: string
  onClick: () => void
}

function ErrorFallback({
  error,
  onRetry,
  exits,
}: {
  error: unknown
  onRetry: () => void
  exits?: Exit[]
}) {
  const { t } = useTranslation()
  const errorMessage = error instanceof Error ? error.message : String(error)
  const errorStack = error instanceof Error ? error.stack : undefined

  return (
    <div role="alert" style={{ padding: 16 }}>
      <h4>{t('shared.feedback.crash.title')}</h4>

      <pre>{errorMessage}</pre>

      <Button variant="contained" size="small" onClick={onRetry}>
        {t('shared.feedback.crash.retry')}
      </Button>
      {exits?.map((exit) => (
        <Button
          key={exit.label}
          size="small"
          sx={{ ml: 1 }}
          onClick={exit.onClick}
        >
          {exit.label}
        </Button>
      ))}

      <details>
        <summary>{t('shared.feedback.crash.details')}</summary>
        <pre>{errorStack}</pre>
      </details>
    </div>
  )
}

interface Props {
  children?: ReactNode
  resetKeys?: unknown[]
  onNavigate?: (path: string) => void
  atHome?: boolean
}

export const BaseErrorBoundary = ({
  children,
  resetKeys,
  onNavigate,
  atHome,
}: Props) => {
  const { t } = useTranslation()
  const exits: Exit[] = !onNavigate
    ? []
    : atHome
      ? [
          {
            label: t('profiles.page.title'),
            onClick: () => onNavigate('/profile'),
          },
          {
            label: t('settings.page.title'),
            onClick: () => onNavigate('/settings'),
          },
        ]
      : [
          {
            label: t('shared.feedback.crash.home'),
            onClick: () => onNavigate('/'),
          },
        ]

  return (
    <ErrorBoundary
      fallbackRender={({ error, resetErrorBoundary }) => (
        <ErrorFallback
          error={error}
          onRetry={resetErrorBoundary}
          exits={exits}
        />
      )}
      resetKeys={resetKeys}
    >
      {children}
    </ErrorBoundary>
  )
}

/**
 * Сбой в рамке окна (уведомления, окна поверх экранов, заголовок): тот же
 * запасной экран, что у страниц. F5 в окне выключен — перезагрузка кнопкой.
 */
export const RouteCrash = () => {
  const { t } = useTranslation()
  const error = useRouteError()
  const navigate = useNavigate()
  const location = useLocation()

  return (
    <ErrorFallback
      error={error}
      onRetry={() =>
        void navigate(location.pathname, { replace: true, state: Date.now() })
      }
      exits={[
        {
          label: t('shared.feedback.crash.reload'),
          onClick: () => window.location.reload(),
        },
      ]}
    />
  )
}
