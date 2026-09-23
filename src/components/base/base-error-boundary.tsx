import { Button } from '@mui/material'
import { ReactNode } from 'react'
import { ErrorBoundary, FallbackProps } from 'react-error-boundary'
import { useTranslation } from 'react-i18next'

function ErrorFallback({
  error,
  resetErrorBoundary,
  onHome,
}: FallbackProps & { onHome?: () => void }) {
  const { t } = useTranslation()
  const errorMessage = error instanceof Error ? error.message : String(error)
  const errorStack = error instanceof Error ? error.stack : undefined

  return (
    <div role="alert" style={{ padding: 16 }}>
      <h4>{t('shared.feedback.crash.title')}</h4>

      <pre>{errorMessage}</pre>

      <Button variant="contained" size="small" onClick={resetErrorBoundary}>
        {t('shared.feedback.crash.retry')}
      </Button>
      {onHome ? (
        <Button size="small" sx={{ ml: 1 }} onClick={onHome}>
          {t('shared.feedback.crash.home')}
        </Button>
      ) : null}

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
  onHome?: () => void
}

export const BaseErrorBoundary = ({ children, resetKeys, onHome }: Props) => {
  return (
    <ErrorBoundary
      fallbackRender={(props) => <ErrorFallback {...props} onHome={onHome} />}
      resetKeys={resetKeys}
    >
      {children}
    </ErrorBoundary>
  )
}
